using System.Diagnostics;
using static RadialMenu.Native;

namespace RadialMenu;

/// <summary>Действия пунктов меню: комбинации клавиш, медиаклавиши, запуск программ.</summary>
internal static class Actions
{
    const uint KEYUP = 2, EXT = 1;
    const byte VK_CONTROL = 0x11, VK_LWIN = 0x5B;

    public static void SendCtrl(byte vk)
    {
        keybd_event(VK_CONTROL, 0, 0, 0);
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYUP, 0);
        keybd_event(VK_CONTROL, 0, KEYUP, 0);
    }

    public static void WinV()
    {
        keybd_event(VK_LWIN, 0, 0, 0);
        keybd_event(0x56, 0, 0, 0);
        keybd_event(0x56, 0, KEYUP, 0);
        keybd_event(VK_LWIN, 0, KEYUP, 0);
    }

    /// <summary>Шаг громкости как клавишей на клавиатуре (Windows сам покажет индикатор).</summary>
    public static void VolumeStep(bool up)
    {
        byte vk = up ? (byte)0xAF : (byte)0xAE;
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYUP, 0);
    }

    public static void MediaKey(byte vk)
    {
        keybd_event(vk, 0, EXT, 0);
        keybd_event(vk, 0, EXT | KEYUP, 0);
    }

    public static void Explorer() => Start("explorer.exe");
    public static void Settings() => Start("ms-settings:");

    public static void Note() => NoteWindow.OpenFromAnyThread();

    static void Start(string target)
    {
        try { Process.Start(new ProcessStartInfo(target) { UseShellExecute = true }); }
        catch (Exception e) { Log.Error("запуск " + target, e); }
    }
}
