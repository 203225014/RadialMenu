using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using static RadialMenu.Native;

namespace RadialMenu;

/// <summary>
/// Само меню: полупрозрачное окно поверх всех (рисуется по пикселям), логика выбора по направлению мыши,
/// анимации. Работает в собственном потоке со своим циклом сообщений.
/// </summary>
internal sealed class Overlay
{
    static Overlay _instance;
    public static void Toast(string text) { if (_instance != null) _instance._toast = (text, Clock() + 0.9); }

    static readonly Stopwatch Sw = Stopwatch.StartNew();
    static double Clock() => Sw.Elapsed.TotalSeconds;

    readonly InputHook _inp;
    readonly Thread _thread;
    volatile bool _quit;

    // окно
    nint _hwnd, _hdcScreen, _hdc, _hbmp, _bits;
    int _half, _size;
    int _winX, _winY;
    bool _visible;
    Bitmap _frame;
    Graphics _g;
    Bitmap _tmp;           // 80×80 для смеси состояний
    readonly ImageAttributes _ia = new();
    readonly ColorMatrix _cm = new();

    // состояние
    double _cx, _cy;
    bool _active, _fading;
    double _px, _py;
    List<Node> _path = new();
    Node _hot;
    Node _dwRoot;
    double _dwT0;
    double _hubP;
    double[] _hubSw;
    (string text, double until)? _toast;
    double _showT, _fadeT, _tPrev, _leakT, _stuckT;
    readonly List<(double at, Action act)> _delayed = new();

    public Overlay(InputHook inp)
    {
        _instance = this;
        _inp = inp;
        _thread = new Thread(Run) { IsBackground = true, Name = "Overlay", Priority = ThreadPriority.AboveNormal };
        _thread.SetApartmentState(ApartmentState.STA);
    }

    public void Start() => _thread.Start();
    public void Stop() { _quit = true; _inp.Signal.Set(); }

    // ------------------------------------------------------------------ цикл
    void Run()
    {
        try
        {
            timeBeginPeriod(1);
            MenuModel.Media.Start();
            _tmp = new Bitmap(Sprites.Box, Sprites.Box, PixelFormat.Format32bppPArgb);
            _ia.SetColorMatrix(_cm);
            Setup();

            while (!_quit)
            {
                if (_rebuild && !_active && !_fading && !_inp.Capture)
                {
                    _rebuild = false;
                    try { Teardown(); Setup(); }
                    catch (Exception e) { Log.Error("пересборка меню", e); }
                }
                while (PeekMessageW(out MSG m, 0, 0, 0, 1)) { TranslateMessage(ref m); DispatchMessageW(ref m); }
                try { Tick(); }
                catch (Exception e) { Log.Error("tick", e); }
                // в покое просыпаемся по событию от хука, во время меню - каждые TickMs
                int wait = (_active || _fading) ? Cfg.TickMs : 40;
                _inp.Signal.WaitOne(wait);
            }
        }
        catch (Exception e) { Log.Error("поток меню", e); }
        finally { timeEndPeriod(1); }
    }

    volatile bool _rebuild;

    /// <summary>Применить новые настройки (размер, набор пунктов): меню пересоберётся, когда оно не на экране.</summary>
    public void Rebuild() { _rebuild = true; _inp.Signal.Set(); }

    void Setup()
    {
        int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1);   // главный монитор
        _cx = sw / 2.0; _cy = sh / 2.0;
        Cfg.Recompute(sw, sh);
        MenuModel.Build();
        _inp.Button = Cfg.Button;
        _half = (int)(Cfg.MainD + Cfg.FanD + Cfg.CircleR * (1 + Cfg.HoverGrow) * 1.2 + 24);
        _size = _half * 2;
        CreateWindow();
        // все кнопки строим заранее, чтобы первый показ не подвисал
        foreach (var n in MenuModel.All())
            for (int s = 0; s < 4; s++) Sprites.Get(s, n.Icon);
        Sprites.Get(0, "play"); Sprites.Get(0, "blank"); Sprites.Dot();
        _path = new List<Node>(); _hot = null; _dwRoot = null; _visible = false;
    }

    void Teardown()
    {
        _g?.Dispose(); _frame?.Dispose();
        foreach (var b in _bytesBmp.Values) b.Dispose();
        _bytesBmp.Clear();
        if (_hwnd != 0) DestroyWindow(_hwnd);
        if (_hdc != 0) DeleteDC(_hdc);
        if (_hbmp != 0) DeleteObject(_hbmp);
        if (_hdcScreen != 0) ReleaseDC(0, _hdcScreen);
        _hwnd = _hdc = _hbmp = _hdcScreen = 0;
    }

    void CreateWindow()
    {
        const uint WS_POPUP = 0x80000000;
        const uint EX = 0x00080000 | 0x00000080 | 0x00000008 | 0x08000000 | 0x00000020; // LAYERED TOOLWINDOW TOPMOST NOACTIVATE TRANSPARENT
        _winX = (int)(_cx - _half); _winY = (int)(_cy - _half);
        _hwnd = CreateWindowExW(EX, "Static", "RadialMenuLayer", WS_POPUP, _winX, _winY, _size, _size, 0, 0, GetModuleHandleW(null), 0);
        if (_hwnd == 0) throw new InvalidOperationException("CreateWindowEx failed");
        _hdcScreen = GetDC(0);
        _hdc = CreateCompatibleDC(_hdcScreen);
        var bmi = new BITMAPINFOHEADER
        {
            biSize = (uint)Marshal.SizeOf<BITMAPINFOHEADER>(), biWidth = _size, biHeight = -_size,
            biPlanes = 1, biBitCount = 32,
        };
        _hbmp = CreateDIBSection(_hdc, ref bmi, 0, out _bits, 0, 0);
        SelectObject(_hdc, _hbmp);
        // рисуем прямо в память DIB-секции
        _frame = new Bitmap(_size, _size, _size * 4, PixelFormat.Format32bppPArgb, _bits);
        _g = Graphics.FromImage(_frame);
        _g.InterpolationMode = InterpolationMode.HighQualityBicubic;
        _g.PixelOffsetMode = PixelOffsetMode.HighQuality;
        _g.CompositingQuality = CompositingQuality.HighSpeed;
    }

    void Present(int alpha)
    {
        var pt = new POINT { X = _winX, Y = _winY };
        var sz = new SIZE { W = _size, H = _size };
        var src = new POINT();
        var bl = new BLENDFUNCTION { BlendOp = 0, BlendFlags = 0, SourceConstantAlpha = (byte)Math.Clamp(alpha, 0, 255), AlphaFormat = 1 };
        UpdateLayeredWindow(_hwnd, _hdcScreen, ref pt, ref sz, _hdc, ref src, 0, ref bl, 2);
    }

    void ShowWin()
    {
        if (_visible) return;
        ShowWindow(_hwnd, 4);  // SW_SHOWNOACTIVATE
        SetWindowPos(_hwnd, HWND_TOPMOST, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
        _visible = true;
    }

    void HideWin()
    {
        if (!_visible) return;
        ShowWindow(_hwnd, 0);
        _visible = false;
    }

    // ------------------------------------------------------------------ события
    void Show()
    {
        _path = new List<Node>();
        _hot = null; _dwRoot = null; _toast = null; _stuckT = Clock();
        _px = _cx; _py = _cy;                // виртуальный указатель в центре
        _inp.Take();
        _active = true; _fading = false;
        var kids = MenuModel.Root.Children;
        for (int i = 0; i < kids.Count; i++) kids[i].P = -0.10 * i;   // круги появляются по очереди
        foreach (var n in MenuModel.All())
        {
            if (n.Parent != MenuModel.Root) n.P = 0;
            n.Hv = 0; n.Sw = null;
        }
        _hubP = 0; _hubSw = null;
        _showT = _tPrev = Clock();
        _g.Clear(Color.Transparent);
        Present(0);
        ShowWin();
    }

    void Hide()
    {
        _active = false;
        _fading = true;   // гаснет плавно, потом HideWin
        _fadeT = Clock();
    }

    IEnumerable<Node> Visible()
    {
        foreach (var n in MenuModel.Root.Children) yield return n;
        foreach (var p in _path) foreach (var c in p.Children) yield return c;
    }

    void Tick()
    {
        double now = Clock();
        while (_inp.Events.TryDequeue(out var ev))
        {
            if (ev == InputEvent.Down && !_active) Show();
            else if ((ev == InputEvent.VolUp || ev == InputEvent.VolDown) && _active)
            {
                Actions.VolumeStep(ev == InputEvent.VolUp);
                _toast = (ev == InputEvent.VolUp ? "Громкость +" : "Громкость −", now + 0.9);
            }
            else if (ev == InputEvent.Up && _active)
            {
                var node = _hot;
                Hide();
                if (node?.Action != null) _delayed.Add((now + 0.03, node.Action));
            }
        }
        for (int i = _delayed.Count - 1; i >= 0; i--)
            if (now >= _delayed[i].at)
            {
                var a = _delayed[i].act;
                _delayed.RemoveAt(i);
                // отдельным потоком: запуск программ (ShellExecute) может занять сотни мс и не должен морозить анимацию
                ThreadPool.QueueUserWorkItem(_ => { try { a(); } catch (Exception e) { Log.Error("действие", e); } });
            }

        // сторож: хук глотает нашу кнопку целиком, так что «зажата» без меню - это утечка (хук не успел ответить);
        // отпускаем её сами, иначе клики перестают работать
        if (!_active && !_inp.Capture)
        {
            int vk = _inp.Button == 1 ? 0x05 : 0x06;
            if ((GetAsyncKeyState(vk) & 0x8000) != 0)
            {
                if (_leakT == 0) _leakT = now;
                else if (now - _leakT > 0.5)
                {
                    _inp.PassInjectUntil = InputHook.Now + 0.5;
                    mouse_event(0x100, 0, 0, (uint)_inp.Button, 0);   // XBUTTONUP
                    _leakT = 0;
                }
            }
            else _leakT = 0;
        }
        // предохранитель: событие «up» могло потеряться - через 20 с меню закрывается само
        if (_active && now - _stuckT > 20)
        {
            _inp.Capture = false;
            _inp.Events.Enqueue(InputEvent.Up);
        }

        if (_active)
        {
            UpdateMenu(now);
            if (GetSystemMetrics(SM_REMOTESESSION) == 0 && !_inp.UseRaw)
            {
                SetCursorPos(_inp.Frozen.x, _inp.Frozen.y);
                _inp.Rebase(_inp.Frozen);
            }
        }
        if (_active || _fading)
        {
            double dt = Math.Min(now - _tPrev, 0.05);
            _tPrev = now;
            Animate(dt);
            double g = _active ? Math.Min(1.0, (now - _showT) / Cfg.FadeIn)
                               : Math.Max(0.0, 1.0 - (now - _fadeT) / Cfg.FadeOut);
            Draw(255 * g);
            if (!_active && g <= 0.0) { _fading = false; HideWin(); }
        }
    }

    // ------------------------------------------------------------------ выбор по направлению
    void UpdateMenu(double now)
    {
        var (dx, dy) = _inp.Take();
        _px += dx * Cfg.Sens;
        _py += dy * Cfg.Sens;
        double ddx = _px - _cx, ddy = _py - _cy;
        double r = Math.Sqrt(ddx * ddx + ddy * ddy);
        double phi = (Math.Atan2(ddy, ddx) * 180 / Math.PI + 360) % 360;
        double rmax = _path.Count > 0 ? Cfg.RMaxSub : Cfg.RMax;
        if (r > rmax)   // указатель (невидимый) не уходит дальше края меню
        {
            _px = _cx + ddx * rmax / r;
            _py = _cy + ddy * rmax / r;
            r = rmax;
        }
        Node hot = null, root = null;
        var kids = MenuModel.Root.Children;
        if (r >= Cfg.Dead)   // блок выбирается по направлению от центра
        {
            root = kids.MinBy(n => MenuModel.AngDiff(phi, n.MidAngle));
            hot = root;
            if (root.Children.Count > 0) MenuModel.LayoutSub(root);
            if (root.Children.Count > 0 && (!root.Dwell || _path.Contains(root)))   // подпункты выбираются, только когда раскрыты
            {
                // если уже стоим на подпункте этого блока - порог выхода из подменю ниже
                bool keep = _hot != null && _hot.Parent == root;
                if (r > Cfg.ROut - (keep ? Cfg.Hyst : 0))
                {
                    // ближайший по углу подпункт; зазоры между секторами не считаются «пустотой»
                    var k = root.Children.MinBy(c => MenuModel.AngDiff(phi, c.MidAngle));
                    if (MenuModel.AngDiff(phi, k.MidAngle) <= (k.A2 - k.A1) / 2 + 1.5) hot = k;
                }
            }
        }
        if (r < Cfg.Dead && _path.Count == 0) _dwRoot = null;   // вернулись в центр, ничего не открыто
        if (r >= Cfg.Dead)   // в центре подменю не трогаем: оно закрывается только переходом на другой блок
        {
            if (root != _dwRoot) { _dwRoot = root; _dwT0 = now; }
            bool opened = _path.Contains(root);
            bool ready = !root.Dwell || opened || now - _dwT0 >= Cfg.DwellT;
            _path = (root.Children.Count > 0 && ready) ? new List<Node> { root } : new List<Node>();
        }
        _hot = hot;
    }

    // ------------------------------------------------------------------ анимация
    static double[] Step(double[] w, int target, double k)
    {
        if (w == null) { w = new double[4]; w[target] = 1; return w; }
        for (int i = 0; i < 4; i++) w[i] += ((i == target ? 1.0 : 0.0) - w[i]) * k;
        return w;
    }

    void Animate(double dt)
    {
        var vis = _active ? new HashSet<Node>(Visible()) : new HashSet<Node>();
        double k = 1 - Math.Exp(-dt / Cfg.ColorT);
        foreach (var n in MenuModel.All())
        {
            if (vis.Contains(n)) n.P = Math.Min(1.0, n.P + dt / Cfg.AppearT);
            else n.P = n.P > 0 ? Math.Max(0.0, n.P - dt / (Cfg.AppearT * 0.6)) : 0.0;
            bool hot = _active && n == _hot && (n.Children.Count == 0 || n.Action != null);
            int target = hot ? 2 : _path.Contains(n) ? 1 : (n.State != null && n.State()) ? 3 : 0;
            n.Sw = Step(n.Sw, target, k);
            n.Hv += ((hot ? 1.0 : 0.0) - n.Hv) * k;
        }
        _hubP = _active ? Math.Min(1.0, _hubP + dt / Cfg.AppearT) : Math.Max(0.0, _hubP - dt / (Cfg.AppearT * 0.6));
        _hubSw = Step(_hubSw, (_hot == null && _active) ? 1 : 0, k);
    }

    // ------------------------------------------------------------------ рисование
    static double EaseBack(double p, double c1 = 1.1)
    {
        p = Math.Clamp(p, 0, 1);
        double c3 = c1 + 1, q = p - 1;
        return 1 + c3 * q * q * q + c1 * q * q;
    }

    /// <summary>Спрайт кнопки как смесь состояний по весам (плавный переход цвета) -> в _tmp.</summary>
    Bitmap Blend(double[] w, string icon)
    {
        double total = 0;
        for (int i = 0; i < 4; i++) if (w[i] > 0.01) total += w[i];
        if (total <= 0) { total = 1; w = new[] { 1.0, 0, 0, 0 }; }
        Array.Clear(_acc);
        for (int s = 0; s < 4; s++)
        {
            if (w[s] <= 0.01) continue;
            float f = (float)(w[s] / total);
            var sp = Sprites.Get(s, icon);
            for (int i = 0; i < sp.Length; i++) _acc[i] += sp[i] * f;
        }
        for (int i = 0; i < _acc.Length; i++) _bytes[i] = (byte)(_acc[i] + 0.5f);
        var data = _tmp.LockBits(new Rectangle(0, 0, Sprites.Box, Sprites.Box), ImageLockMode.WriteOnly, PixelFormat.Format32bppPArgb);
        try { Marshal.Copy(_bytes, 0, data.Scan0, _bytes.Length); }
        finally { _tmp.UnlockBits(data); }
        return _tmp;
    }

    readonly float[] _acc = new float[Sprites.Box * Sprites.Box * 4];
    readonly byte[] _bytes = new byte[Sprites.Box * Sprites.Box * 4];
    readonly Dictionary<byte[], Bitmap> _bytesBmp = new();

    void Put(Image img, int size, double x, double y, double alpha)
    {
        if (size < 6) return;
        _cm.Matrix33 = (float)Math.Clamp(alpha, 0, 1);
        _ia.SetColorMatrix(_cm);
        var dest = new Rectangle((int)Math.Round(x - size / 2.0), (int)Math.Round(y - size / 2.0), size, size);
        _g.DrawImage(img, dest, 0, 0, img.Width, img.Height, GraphicsUnit.Pixel, _ia);
    }

    void PutBytes(byte[] px, int w, int size, double x, double y, double alpha)
    {
        if (!_bytesBmp.TryGetValue(px, out var bmp))
        {
            bmp = new Bitmap(w, w, PixelFormat.Format32bppPArgb);
            var data = bmp.LockBits(new Rectangle(0, 0, w, w), ImageLockMode.WriteOnly, PixelFormat.Format32bppPArgb);
            Marshal.Copy(px, 0, data.Scan0, px.Length);
            bmp.UnlockBits(data);
            _bytesBmp[px] = bmp;
        }
        Put(bmp, size, x, y, alpha);
    }

    void Draw(double alpha255)
    {
        int H = _half;
        _g.Clear(Color.Transparent);
        var pos = new Dictionary<Node, (double x, double y)>();
        (double x, double y) PosOf(Node n)
        {
            if (pos.TryGetValue(n, out var p)) return p;
            double fx = H + n.OffX, fy = H + n.OffY;
            var (sx, sy) = n.Parent == MenuModel.Root ? ((double)H, (double)H) : PosOf(n.Parent);
            double e = EaseBack(n.P);
            return pos[n] = (sx + (fx - sx) * e, sy + (fy - sy) * e);
        }

        var nodes = MenuModel.All().Where(n => n.P > 0.01).ToList();

        // центр: маленькая серая кнопка и синяя точка - куда сейчас смотрит мышь
        double hp = EaseBack(_hubP);
        int hs = (int)Math.Round(Sprites.Box * (Cfg.HubR * hp) / (Sprites.Diam / 2));
        if (hs >= 6)
        {
            double a = Math.Min(1.0, _hubP * 2.5);
            PutBytes(Sprites.Get(0, "blank"), Sprites.Box, hs, H, H, a);
            double dx = _px - _cx, dy = _py - _cy, L = Math.Sqrt(dx * dx + dy * dy), reach = Cfg.HubR * 0.45;
            if (L > reach) { dx = dx * reach / L; dy = dy * reach / L; }
            int ds = (int)Math.Round(Sprites.DotBox * hp);
            if (ds >= 4) PutBytes(Sprites.Dot(), Sprites.DotBox, ds, H + dx, H + dy, a);
        }
        // кнопки
        foreach (var n in nodes)
        {
            double e = EaseBack(n.P);
            double rad = Math.Max(0.0, Cfg.CircleR * (1 + Cfg.HoverGrow * n.Hv) * e);
            string icon = n.Dyn != null ? n.Dyn().icon : n.Icon;
            var (x, y) = PosOf(n);
            var sp = Blend(n.Sw, icon);
            Put(sp, (int)Math.Round(Sprites.Box * rad / (Sprites.Diam / 2)), x, y, Math.Min(1.0, Math.Max(0.0, n.P * 2.5)));
        }
        // подпись выбранной кнопки - со стороны центра, в промежутке между кнопками
        foreach (var n in nodes)
        {
            if (n.Hv <= 0.15) continue;
            string label = n.Dyn != null ? n.Dyn().label : n.Label;
            if (n.State != null) label += n.State() ? " · играет" : " · выкл";
            var (x, y) = PosOf(n);
            var pill = Sprites.Pill(label);
            double rad = Cfg.CircleR * (1 + Cfg.HoverGrow * n.Hv);
            double py = y <= H ? y + rad + 6 : y - rad - 6 - pill.Height;
            double px = Math.Clamp(x - pill.Width / 2.0, 2, _size - pill.Width - 2);
            py = Math.Clamp(py, 2, _size - pill.Height - 2);
            DrawPill(pill, px, py, Math.Min(1.0, n.Hv * 1.5));
        }
        // подпись при смене громкости - под центром, пока не погаснет
        if (_toast is { } tst)
        {
            var (text, until) = tst;
            double left = until - Clock();
            if (left <= 0) _toast = null;
            else
            {
                var pill = Sprites.Pill(text, 14);
                DrawPill(pill, H - pill.Width / 2.0, H + Cfg.HubR + 14, Math.Min(1.0, left / 0.3));
            }
        }
        Present((int)alpha255);
    }

    void DrawPill(Bitmap pill, double x, double y, double alpha)
    {
        _cm.Matrix33 = (float)Math.Clamp(alpha, 0, 1);
        _ia.SetColorMatrix(_cm);
        _g.DrawImage(pill, new Rectangle((int)x, (int)y, pill.Width, pill.Height), 0, 0, pill.Width, pill.Height, GraphicsUnit.Pixel, _ia);
    }
}
