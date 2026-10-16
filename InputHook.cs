using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.InteropServices;
using static RadialMenu.Native;

namespace RadialMenu;

internal enum InputEvent { Down, Up, VolUp, VolDown }

/// <summary>
/// Низкоуровневый хук мыши (отдельный поток с высоким приоритетом): глотает боковую кнопку, колесо и движение,
/// пока кнопка зажата. Дельты движения берутся из Raw Input (курсор при этом стоит на месте);
/// по RDP raw-дельт нет, там считаем по координатам указателя.
/// </summary>
internal sealed unsafe class InputHook
{
    public readonly ConcurrentQueue<InputEvent> Events = new();
    public readonly AutoResetEvent Signal = new(false);
    public volatile bool Capture;
    public volatile bool Enabled = true;     // false = пауза: все события мыши проходят как обычно
    public volatile bool UseRaw;
    public int Button;                       // 1 или 2
    public (int x, int y) Frozen;            // где нажали кнопку
    public double PassInjectUntil;           // до этого момента синтетические события кнопки не глотаем

    readonly object _lock = new();
    int _dx, _dy;
    (int x, int y) _last;
    bool _rawOk = true;
    nint _hook;
    HookProc _hookProc;
    WndProc _wndProc;
    readonly Thread _thread;

    public static double Now => Stopwatch.GetTimestamp() / (double)Stopwatch.Frequency;

    public InputHook(int button)
    {
        Button = button;
        UseRaw = GetSystemMetrics(SM_REMOTESESSION) == 0;
        _thread = new Thread(Run) { IsBackground = true, Name = "MouseHook", Priority = ThreadPriority.Highest };
    }

    public void Start() => _thread.Start();
    void Post(InputEvent e) { Events.Enqueue(e); Signal.Set(); }

    public (int dx, int dy) Take()
    {
        lock (_lock)
        {
            var d = (_dx, _dy);
            _dx = _dy = 0;
            return d;
        }
    }

    public void Rebase((int x, int y) pos) { lock (_lock) _last = pos; }

    void Install()
    {
        _hook = SetWindowsHookExW(14, _hookProc, GetModuleHandleW(null), 0);
        if (_hook == 0) throw new InvalidOperationException("SetWindowsHookEx failed: " + Marshal.GetLastWin32Error());
    }

    void Run()
    {
        try
        {
            _hookProc = HookCallback;
            Install();
            SetTimer(0, 0, 2000, 0);   // Windows мог снять хук после долгого ответа - раз в 2 с ставим заново
            if (UseRaw)
            {
                try { SetupRaw(); }
                catch (Exception e) { Log.Error("raw input", e); _rawOk = UseRaw = false; }
            }
            while (GetMessageW(out MSG msg, 0, 0, 0) > 0)
            {
                if (msg.message == WM_TIMER)
                {
                    if (!Capture)
                    {
                        try { UnhookWindowsHookEx(_hook); Install(); }
                        catch (Exception e) { Log.Error("re-hook", e); }
                    }
                    continue;
                }
                TranslateMessage(ref msg);
                DispatchMessageW(ref msg);
            }
        }
        catch (Exception e) { Log.Error("поток хука", e); }
    }

    nint HookCallback(int nCode, nint wParam, nint lParam)
    {
        try
        {
            if (nCode == 0 && (Enabled || Capture))
            {
                var info = (MSLLHOOKSTRUCT*)lParam;
                int w = (int)wParam;
                if (w == WM_XBUTTONDOWN || w == WM_XBUTTONUP)
                {
                    if (((info->mouseData >> 16) & 0xFFFF) == (uint)Button)
                    {
                        if ((info->flags & 1) != 0 && Now < PassInjectUntil)
                            return CallNextHookEx(0, nCode, wParam, lParam);
                        if (w == WM_XBUTTONDOWN)
                        {
                            lock (_lock)
                            {
                                _dx = _dy = 0;
                                Frozen = _last = (info->pt.X, info->pt.Y);
                            }
                            // режим выбираем на каждое нажатие: RDP мог подключиться позже запуска
                            UseRaw = _rawOk && GetSystemMetrics(SM_REMOTESESSION) == 0;
                            Capture = true;
                            Post(InputEvent.Down);
                        }
                        else
                        {
                            Capture = false;
                            Post(InputEvent.Up);
                        }
                        return 1;
                    }
                }
                else if (w == WM_MOUSEWHEEL && Capture)
                {
                    short delta = (short)((info->mouseData >> 16) & 0xFFFF);
                    if (delta != 0) Post(delta > 0 ? InputEvent.VolUp : InputEvent.VolDown);
                    return 1;   // колесо в это время не крутит страницы
                }
                else if (w == WM_MOUSEMOVE && Capture)
                {
                    if (UseRaw) return 1;   // курсор остаётся на месте
                    bool own = (info->flags & 1) != 0 && info->pt.X == Frozen.x && info->pt.Y == Frozen.y;  // наш SetCursorPos
                    if (!own)
                    {
                        lock (_lock)
                        {
                            _dx += info->pt.X - _last.x;
                            _dy += info->pt.Y - _last.y;
                            _last = (info->pt.X, info->pt.Y);
                        }
                    }
                }
            }
        }
        catch { /* хук не должен падать */ }
        return CallNextHookEx(0, nCode, wParam, lParam);
    }

    void SetupRaw()
    {
        _wndProc = RawWndProc;
        nint hinst = GetModuleHandleW(null);
        var wc = new WNDCLASS
        {
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_wndProc),
            hInstance = hinst,
            lpszClassName = "RadialMenuRawInput",
        };
        RegisterClassW(ref wc);
        nint hwnd = CreateWindowExW(0, "RadialMenuRawInput", "", 0, 0, 0, 0, 0, HWND_MESSAGE, 0, hinst, 0);
        var rid = new[] { new RAWINPUTDEVICE { usUsagePage = 1, usUsage = 2, dwFlags = 0x100, hwndTarget = hwnd } };  // мышь, INPUTSINK
        if (hwnd == 0 || !RegisterRawInputDevices(rid, 1, (uint)sizeof(RAWINPUTDEVICE)))
            throw new InvalidOperationException("raw input");
    }

    nint RawWndProc(nint hwnd, uint msg, nint wParam, nint lParam)
    {
        if (msg == WM_INPUT)
        {
            if (Capture)
            {
                uint size = 0;
                GetRawInputData(lParam, 0x10000003, 0, ref size, 24);
                if (size is > 0 and <= 256)
                {
                    byte* buf = stackalloc byte[(int)size];
                    if (GetRawInputData(lParam, 0x10000003, (nint)buf, ref size, 24) == size && *(uint*)buf == 0)  // RIM_TYPEMOUSE
                    {
                        ushort flags = *(ushort*)(buf + 24);
                        if ((flags & 1) == 0)   // относительное движение
                        {
                            int x = *(int*)(buf + 36), y = *(int*)(buf + 40);
                            lock (_lock) { _dx += x; _dy += y; }
                        }
                    }
                }
            }
            return 0;
        }
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}
