using System.Drawing;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Forms;

namespace RadialMenu;

/// <summary>
/// Мини-окно быстрой заметки для Obsidian: текст -> Ctrl+Enter -> файл Ring_NN.md в корне хранилища.
/// Esc - отмена. Как в Obsidian: набрал [[ - список заметок, стрелки/Enter/Tab подставляют ссылку, ]] ставится сам.
/// </summary>
internal sealed class NoteWindow : Form
{
    static readonly Color BG = Color.FromArgb(0x1b, 0x1f, 0x2b), BG2 = Color.FromArgb(0x26, 0x2c, 0x3d),
        EDGE = Color.FromArgb(0x8f, 0xa6, 0xd6), DIM = Color.FromArgb(0x87, 0x91, 0xa8), ACC = Color.FromArgb(0x69, 0x9b, 0xff);
    static readonly Regex LinkRe = new(@"\[\[([^\[\]|#\n]*)$", RegexOptions.Compiled);
    static readonly Regex RingRe = new(@"^Ring_(\d+)\.md$", RegexOptions.IgnoreCase | RegexOptions.Compiled);

    /// <summary>Скрытый элемент главного потока: через него другие потоки открывают окно.</summary>
    public static Control Ui;
    static NoteWindow _current;

    public static void OpenFromAnyThread() => Ui?.BeginInvoke(new Action(Open));

    static void Open()
    {
        if (_current != null && !_current.IsDisposed) { _current.Grab(); return; }
        _current = new NoteWindow();
        _current.Show();
        _current.Grab();
    }

    // ------------------------------------------------------------------ окно
    readonly string _vault;
    readonly bool _ok;
    readonly List<(string name, string link, DateTime mtime)> _names;
    List<(string name, string link, DateTime mtime)> _items = new();
    readonly Label _title;
    readonly TextBox _text;
    readonly ListBox _list;
    readonly Panel _body;
    readonly Label _foot;
    Point _dragFrom;

    NoteWindow()
    {
        _vault = Cfg.Vault;
        _ok = !string.IsNullOrWhiteSpace(_vault) && Directory.Exists(_vault);
        _names = _ok ? ScanNames(_vault) : new();

        float k = DeviceDpi / 96f;
        int S(int v) => (int)Math.Round(v * k);
        AutoScaleMode = AutoScaleMode.None;
        FormBorderStyle = FormBorderStyle.None;
        ShowInTaskbar = false;
        TopMost = true;
        StartPosition = FormStartPosition.Manual;
        Opacity = 0.94;
        BackColor = EDGE;
        Padding = new Padding(1);
        Size = new Size(S(500), S(270));
        var scr = Screen.PrimaryScreen.Bounds;
        Location = new Point(scr.X + (scr.Width - Width) / 2, scr.Y + (scr.Height - Height) / 2 - S(40));

        _body = new Panel { Dock = DockStyle.Fill, BackColor = BG };
        Controls.Add(_body);

        var fTitle = new Font("Segoe UI", 11, FontStyle.Bold);
        var fText = new Font("Segoe UI", 12);
        var fSmall = new Font("Segoe UI", 9);

        var head = new Panel { Dock = DockStyle.Top, Height = S(36), BackColor = BG, Padding = new Padding(S(14), S(10), 0, 0) };
        _title = new Label { AutoSize = true, ForeColor = ACC, Font = fTitle, Dock = DockStyle.Left, TextAlign = ContentAlignment.MiddleLeft,
            Text = _ok ? RingName(NextRingNumber(_vault)) : "Нет хранилища" };
        var sub = new Label { AutoSize = true, ForeColor = DIM, Font = fSmall, Dock = DockStyle.Left, Text = "  новая заметка",
            TextAlign = ContentAlignment.MiddleLeft };
        head.Controls.Add(sub);
        head.Controls.Add(_title);
        foreach (Control c in new Control[] { head, _title, sub })
        {
            c.MouseDown += (_, e) => _dragFrom = e.Location + (Size)(c == head ? Point.Empty : c.Location);
            c.MouseMove += (_, e) =>
            {
                if (e.Button != MouseButtons.Left) return;
                var p = MousePosition;
                Location = new Point(p.X - _dragFrom.X, p.Y - _dragFrom.Y);
            };
        }

        _foot = new Label { Dock = DockStyle.Bottom, Height = S(26), ForeColor = DIM, Font = fSmall, BackColor = BG,
            Padding = new Padding(S(14), S(2), 0, 0), Text = "Ctrl+Enter - сохранить    Esc - отмена    [[ - ссылка на заметку" };

        var wrap = new Panel { Dock = DockStyle.Fill, BackColor = BG, Padding = new Padding(S(14), S(4), S(14), S(4)) };
        _text = new TextBox
        {
            Dock = DockStyle.Fill, Multiline = true, BorderStyle = BorderStyle.None, BackColor = BG, ForeColor = Color.White,
            Font = fText, AcceptsReturn = true, WordWrap = true, ScrollBars = ScrollBars.None,
        };
        wrap.Controls.Add(_text);
        _body.Controls.Add(wrap);
        _body.Controls.Add(_foot);
        _body.Controls.Add(head);

        _list = new ListBox
        {
            Visible = false, BackColor = BG2, ForeColor = Color.White, Font = fText, BorderStyle = BorderStyle.FixedSingle,
            IntegralHeight = false, Width = S(260), DrawMode = DrawMode.OwnerDrawFixed, ItemHeight = S(26),
        };
        _list.DrawItem += (_, e) =>
        {
            if (e.Index < 0) return;
            bool sel = (e.State & DrawItemState.Selected) != 0;
            using (var br = new SolidBrush(sel ? ACC : BG2)) e.Graphics.FillRectangle(br, e.Bounds);
            TextRenderer.DrawText(e.Graphics, (string)_list.Items[e.Index], fText, new Rectangle(e.Bounds.X + 8, e.Bounds.Y, e.Bounds.Width - 8, e.Bounds.Height),
                Color.White, TextFormatFlags.VerticalCenter | TextFormatFlags.Left | TextFormatFlags.EndEllipsis);
        };
        _list.MouseUp += (_, _) => Accept();
        _body.Controls.Add(_list);
        _list.BringToFront();

        if (!_ok)
        {
            _text.Text = "Папка хранилища не найдена:\r\n" + (string.IsNullOrWhiteSpace(_vault) ? "(не задана)" : _vault) + "\r\n\r\nУкажите её в настройках (иконка в трее).";
            _text.ReadOnly = true;
        }

        _text.KeyDown += OnKeyDown;
        _text.KeyPress += OnKeyPress;
        _text.TextChanged += (_, _) => UpdateSuggest();
        _text.KeyUp += (_, e) => { if (e.KeyCode is Keys.Left or Keys.Right or Keys.Home or Keys.End) UpdateSuggest(); };
        _text.MouseUp += (_, _) => UpdateSuggest();
        FormClosed += (_, _) => { if (_current == this) _current = null; };
    }

    [DllImport("user32.dll")] static extern nint GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint hwnd, nint pid);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(nint hwnd);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();

    /// <summary>Забрать клавиатурный фокус (Windows не любит, когда фокус «крадут»).</summary>
    void Grab()
    {
        uint cur = GetCurrentThreadId();
        uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), 0);
        try
        {
            AttachThreadInput(fgThread, cur, true);
            SetForegroundWindow(Handle);
            BringToFront();
        }
        finally { AttachThreadInput(fgThread, cur, false); }
        Activate();
        _text.Focus();
    }

    // ------------------------------------------------------------------ клавиши
    void OnKeyDown(object s, KeyEventArgs e)
    {
        if (_list.Visible)
        {
            if (e.KeyCode == Keys.Up) { MoveSel(-1); Eat(e); return; }
            if (e.KeyCode == Keys.Down) { MoveSel(1); Eat(e); return; }
            if (e.KeyCode is Keys.Return or Keys.Tab) { Accept(); Eat(e); return; }
            if (e.KeyCode == Keys.Escape) { _list.Visible = false; Eat(e); return; }
        }
        if (e.KeyCode == Keys.Return && e.Control) { Save(); Eat(e); return; }
        if (e.KeyCode == Keys.Escape) { Close(); Eat(e); return; }
        if (e.KeyCode == Keys.A && e.Control) { _text.SelectAll(); Eat(e); }
    }

    static void Eat(KeyEventArgs e) { e.Handled = true; e.SuppressKeyPress = true; }

    void OnKeyPress(object s, KeyPressEventArgs e)
    {
        // [[ -> [[]] с курсором внутри
        if (e.KeyChar == '[' && _text.SelectionStart > 0 && _text.Text[_text.SelectionStart - 1] == '[' && !_text.ReadOnly)
        {
            e.Handled = true;
            _text.SelectedText = "[]]";
            _text.SelectionStart -= 2;
            UpdateSuggest();
        }
    }

    // ------------------------------------------------------------------ подсказки [[
    void UpdateSuggest()
    {
        if (!_ok || _text.ReadOnly) return;
        string before = LineBeforeCaret(out _);
        var m = LinkRe.Match(before);
        if (!m.Success) { _list.Visible = false; return; }
        string q = m.Groups[1].Value.ToLowerInvariant();
        _items = _names.Where(n => n.name.ToLowerInvariant().Contains(q))
            .OrderBy(n => !n.name.ToLowerInvariant().StartsWith(q)).Take(80).ToList();
        if (_items.Count == 0) { _list.Visible = false; return; }
        _list.BeginUpdate();
        _list.Items.Clear();
        foreach (var n in _items) _list.Items.Add(n.name);
        _list.EndUpdate();
        _list.SelectedIndex = 0;
        int rows = Math.Min(6, _items.Count);
        _list.Height = _list.ItemHeight * rows + 6;
        PlaceList();
        _list.Visible = true;
        _list.BringToFront();
    }

    string LineBeforeCaret(out int lineStart)
    {
        int caret = _text.SelectionStart;
        lineStart = caret == 0 ? 0 : _text.Text.LastIndexOf('\n', caret - 1) + 1;
        return _text.Text.Substring(lineStart, caret - lineStart);
    }

    void PlaceList()
    {
        var p = _text.GetPositionFromCharIndex(Math.Max(0, _text.SelectionStart - 1));
        var screen = _text.PointToScreen(new Point(p.X, p.Y + _text.Font.Height + 2));
        var pt = _body.PointToClient(screen);
        int x = Math.Max(4, Math.Min(pt.X, _body.Width - _list.Width - 4));
        int y = pt.Y;
        if (y + _list.Height > _body.Height - 4)   // не помещается снизу - над строкой
            y = Math.Max(4, pt.Y - _text.Font.Height - 2 - _list.Height);
        _list.Location = new Point(x, y);
    }

    void MoveSel(int d)
    {
        if (_list.Items.Count == 0) return;
        _list.SelectedIndex = Math.Clamp(_list.SelectedIndex + d, 0, _list.Items.Count - 1);
    }

    void Accept()
    {
        int idx = _list.SelectedIndex;
        if (idx < 0 || idx >= _items.Count) { _list.Visible = false; return; }
        string link = _items[idx].link;
        var m = LinkRe.Match(LineBeforeCaret(out int lineStart));
        if (!m.Success) { _list.Visible = false; return; }
        int caret = _text.SelectionStart;
        int start = lineStart + m.Groups[1].Index;
        _text.Select(start, caret - start);
        _text.SelectedText = link;
        int pos = _text.SelectionStart;
        if (pos + 2 <= _text.TextLength && _text.Text.Substring(pos, 2) == "]]") _text.SelectionStart = pos + 2;
        else { _text.SelectedText = "]]"; }
        _list.Visible = false;
        _text.Focus();
    }

    // ------------------------------------------------------------------ сохранение
    void Save()
    {
        if (!_ok) { Close(); return; }
        string content = _text.Text.Replace("\r\n", "\n").TrimEnd();
        if (content.Length == 0) { Close(); return; }
        string name = null;
        for (int i = 0; i < 20 && name == null; i++)   // на случай гонки с другим файлом
        {
            string n = RingName(NextRingNumber(_vault));
            try
            {
                using var fs = new FileStream(Path.Combine(_vault, n + ".md"), FileMode.CreateNew, FileAccess.Write);
                fs.Write(new UTF8Encoding(false).GetBytes(content + "\n"));
                name = n;
            }
            catch (IOException) when (File.Exists(Path.Combine(_vault, n + ".md"))) { }
            catch (Exception e) { Log.Error("сохранение заметки", e); break; }
        }
        _list.Visible = false;
        _text.Enabled = false;
        _title.Text = name != null ? "Сохранено: " + name : "Не удалось сохранить";
        _title.ForeColor = name != null ? Color.FromArgb(0x7f, 0xe0, 0xa3) : Color.FromArgb(0xff, 0x8a, 0x8a);
        var t = new System.Windows.Forms.Timer { Interval = 650 };
        t.Tick += (_, _) => { t.Stop(); t.Dispose(); Close(); };
        t.Start();
    }

    // ------------------------------------------------------------------ хранилище
    static string RingName(int n) => $"Ring_{n:00}";

    static int NextRingNumber(string vault)
    {
        int mx = 0;
        foreach (var f in Walk(vault, "*.md"))
        {
            var m = RingRe.Match(Path.GetFileName(f));
            if (m.Success && int.TryParse(m.Groups[1].Value, out int v)) mx = Math.Max(mx, v);
        }
        return mx + 1;
    }

    /// <summary>Файлы по маске, без скрытых папок (.obsidian, .git, ...).</summary>
    static IEnumerable<string> Walk(string dir, string pattern)
    {
        var stack = new Stack<string>();
        stack.Push(dir);
        while (stack.Count > 0)
        {
            string d = stack.Pop();
            string[] files = Array.Empty<string>(), subs = Array.Empty<string>();
            try { files = Directory.GetFiles(d, pattern); subs = Directory.GetDirectories(d); } catch { }
            foreach (var f in files) yield return f;
            foreach (var s in subs) if (!Path.GetFileName(s).StartsWith('.')) stack.Push(s);
        }
    }

    /// <summary>Заметки .md и .canvas: (имя, текст ссылки, время изменения), свежие выше.</summary>
    static List<(string name, string link, DateTime mtime)> ScanNames(string vault)
    {
        var found = new List<(string b, string rel, DateTime mt)>();
        foreach (var f in Walk(vault, "*"))
        {
            string ext = Path.GetExtension(f).ToLowerInvariant();
            if (ext != ".md" && ext != ".canvas") continue;
            string fn = Path.GetFileName(f);
            string b = ext == ".md" ? fn[..^3] : fn;   // у canvas в ссылке остаётся расширение
            DateTime mt = DateTime.MinValue;
            try { mt = File.GetLastWriteTime(f); } catch { }
            found.Add((b, Path.GetRelativePath(vault, f).Replace('\\', '/'), mt));
        }
        var count = found.GroupBy(x => x.b).ToDictionary(g => g.Key, g => g.Count());
        return found.Select(x => (x.b, count[x.b] == 1 ? x.b : (x.rel.EndsWith(".md", StringComparison.OrdinalIgnoreCase) ? x.rel[..^3] : x.rel), x.mt))
            .OrderByDescending(x => x.Item3).ToList();
    }
}
