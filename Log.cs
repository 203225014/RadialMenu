using System.Text;

namespace RadialMenu;

internal static class Log
{
    static readonly object Gate = new();
    public static readonly string Path = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "RadialMenu_error.log");

    public static void Error(string where, Exception e)
    {
        try
        {
            lock (Gate)
                File.AppendAllText(Path, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss} {where}\n{e}\n\n", Encoding.UTF8);
        }
        catch { }
    }
}
