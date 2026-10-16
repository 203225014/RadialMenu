using System.Runtime.InteropServices;

namespace RadialMenu;

internal static class Native
{
    public delegate nint HookProc(int nCode, nint wParam, nint lParam);
    public delegate nint WndProc(nint hwnd, uint msg, nint wParam, nint lParam);

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct SIZE { public int W, H; }

    [StructLayout(LayoutKind.Sequential)]
    public struct MSLLHOOKSTRUCT
    {
        public POINT pt;
        public uint mouseData, flags, time;
        public nint extra;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MSG
    {
        public nint hwnd;
        public uint message;
        public nint wParam, lParam;
        public uint time;
        public POINT pt;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct WNDCLASS
    {
        public uint style;
        public nint lpfnWndProc;
        public int cbClsExtra, cbWndExtra;
        public nint hInstance, hIcon, hCursor, hbrBackground;
        public string lpszMenuName, lpszClassName;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct RAWINPUTDEVICE
    {
        public ushort usUsagePage, usUsage;
        public uint dwFlags;
        public nint hwndTarget;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct BITMAPINFOHEADER
    {
        public uint biSize;
        public int biWidth, biHeight;
        public ushort biPlanes, biBitCount;
        public uint biCompression, biSizeImage;
        public int biXPelsPerMeter, biYPelsPerMeter;
        public uint biClrUsed, biClrImportant;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct BLENDFUNCTION
    {
        public byte BlendOp, BlendFlags, SourceConstantAlpha, AlphaFormat;
    }

    // ---- user32
    [DllImport("user32.dll", SetLastError = true)]
    public static extern nint SetWindowsHookExW(int idHook, HookProc fn, nint hMod, uint threadId);
    [DllImport("user32.dll")]
    public static extern bool UnhookWindowsHookEx(nint hook);
    [DllImport("user32.dll")]
    public static extern nint CallNextHookEx(nint hook, int nCode, nint wParam, nint lParam);
    [DllImport("user32.dll")]
    public static extern int GetMessageW(out MSG msg, nint hwnd, uint min, uint max);
    [DllImport("user32.dll")]
    public static extern bool PeekMessageW(out MSG msg, nint hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")]
    public static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")]
    public static extern nint DispatchMessageW(ref MSG msg);
    [DllImport("user32.dll")]
    public static extern nuint SetTimer(nint hwnd, nuint id, uint ms, nint proc);
    [DllImport("user32.dll")]
    public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")]
    public static extern short GetAsyncKeyState(int vk);
    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, int dx, int dy, uint data, nint extra);
    [DllImport("user32.dll")]
    public static extern void keybd_event(byte vk, byte scan, uint flags, nint extra);
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern ushort RegisterClassW(ref WNDCLASS wc);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern nint CreateWindowExW(uint exStyle, string cls, string title, uint style,
        int x, int y, int w, int h, nint parent, nint menu, nint inst, nint param);
    [DllImport("user32.dll")]
    public static extern nint DefWindowProcW(nint hwnd, uint msg, nint wParam, nint lParam);
    [DllImport("user32.dll")]
    public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] devices, uint count, uint size);
    [DllImport("user32.dll")]
    public static extern uint GetRawInputData(nint hRaw, uint cmd, nint data, ref uint size, uint headerSize);
    [DllImport("user32.dll")]
    public static extern nint GetDC(nint hwnd);
    [DllImport("user32.dll")]
    public static extern int ReleaseDC(nint hwnd, nint dc);
    [DllImport("user32.dll")]
    public static extern bool UpdateLayeredWindow(nint hwnd, nint dstDc, ref POINT dst, ref SIZE size,
        nint srcDc, ref POINT src, uint key, ref BLENDFUNCTION blend, uint flags);
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(nint hwnd, int cmd);
    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")]
    public static extern bool DestroyWindow(nint hwnd);

    // ---- gdi32
    [DllImport("gdi32.dll")]
    public static extern nint CreateCompatibleDC(nint dc);
    [DllImport("gdi32.dll")]
    public static extern nint CreateDIBSection(nint dc, ref BITMAPINFOHEADER bmi, uint usage, out nint bits, nint section, uint offset);
    [DllImport("gdi32.dll")]
    public static extern nint SelectObject(nint dc, nint obj);
    [DllImport("gdi32.dll")]
    public static extern bool DeleteObject(nint obj);
    [DllImport("gdi32.dll")]
    public static extern bool DeleteDC(nint dc);

    // ---- kernel32 / winmm
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern nint GetModuleHandleW(string name);
    [DllImport("winmm.dll")]
    public static extern uint timeBeginPeriod(uint ms);
    [DllImport("winmm.dll")]
    public static extern uint timeEndPeriod(uint ms);
    [DllImport("user32.dll")]
    public static extern bool SystemParametersInfoW(uint action, uint param, nint data, uint flags);

    public const int WM_MOUSEMOVE = 0x0200, WM_MOUSEWHEEL = 0x020A, WM_XBUTTONDOWN = 0x020B, WM_XBUTTONUP = 0x020C;
    public const int WM_INPUT = 0x00FF, WM_TIMER = 0x0113;
    public const int SM_REMOTESESSION = 0x1000;
    public static readonly nint HWND_MESSAGE = -3, HWND_TOPMOST = -1;
}
