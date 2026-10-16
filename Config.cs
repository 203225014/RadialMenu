using System.Text.Json;
using Microsoft.Win32;

namespace RadialMenu;

/// <summary>Пользовательские настройки (хранятся в %AppData%\RadialMenu\settings.json).</summary>
internal sealed class Settings
{
    public int Button { get; set; } = 1;              // 1 = «назад» (XButton1), 2 = «вперёд» (XButton2)
    public string Vault { get; set; } = "";           // папка хранилища Obsidian
    public int SensPercent { get; set; } = 100;       // чувствительность, %
    public int SizePercent { get; set; } = 100;       // размер меню, %
    public int DwellMs { get; set; } = 450;           // задержка раскрытия «Копировать»/«Вставить»
    public List<string> DisabledItems { get; set; } = new();   // id отключённых пунктов

    public static string Dir => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "RadialMenu");
    public static string FilePath => Path.Combine(Dir, "settings.json");

    public static Settings Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var s = JsonSerializer.Deserialize<Settings>(File.ReadAllText(FilePath)) ?? new Settings();
                s.Normalize();
                return s;
            }
        }
        catch (Exception e) { Log.Error("чтение настроек", e); }
        var d = new Settings();
        // первый запуск: подхватить хранилище, если оно лежит по привычному пути
        if (Directory.Exists(@"C:\Users\HP\Work")) d.Vault = @"C:\Users\HP\Work";
        return d;
    }

    public void Save()
    {
        try
        {
            Normalize();
            Directory.CreateDirectory(Dir);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch (Exception e) { Log.Error("сохранение настроек", e); }
    }

    void Normalize()
    {
        if (Button is not (1 or 2)) Button = 1;
        SensPercent = Math.Clamp(SensPercent, 20, 300);
        SizePercent = Math.Clamp(SizePercent, 50, 150);
        DwellMs = Math.Clamp(DwellMs, 100, 2000);
        DisabledItems ??= new();
        Vault ??= "";
    }

    public Settings Clone()
    {
        var c = (Settings)MemberwiseClone();
        c.DisabledItems = new List<string>(DisabledItems);
        return c;
    }
}

/// <summary>Автозапуск вместе с Windows (раздел Run текущего пользователя).</summary>
internal static class Autostart
{
    const string KeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
    const string ValueName = "RadialMenu";

    public static bool IsEnabled
    {
        get
        {
            try { using var k = Registry.CurrentUser.OpenSubKey(KeyPath); return k?.GetValue(ValueName) != null; }
            catch { return false; }
        }
    }

    public static void Set(bool on)
    {
        try
        {
            using var k = Registry.CurrentUser.CreateSubKey(KeyPath);
            if (on) k.SetValue(ValueName, "\"" + Environment.ProcessPath + "\"");
            else k.DeleteValue(ValueName, false);
        }
        catch (Exception e) { Log.Error("автозапуск", e); }
    }
}

/// <summary>Рабочие параметры меню: базовые размеры из макета, масштабированные настройками и экраном.</summary>
internal static class Cfg
{
    /// <summary>Версия берётся из csproj (&lt;Version&gt;), единственное место, где её надо менять.</summary>
    public static readonly string Version =
        typeof(Cfg).Assembly.GetName().Version is { } v ? $"{v.Major}.{v.Minor}.{v.Build}" : "0.0.0";

    public static Settings S = new();

    public static int Button => S.Button;
    public static string Vault => S.Vault;

    public static double MainD, FanD, ROut, RMax, RMaxSub, Dead, CircleR, HubR;
    public static double Sens, DwellT;
    public const double FanSpread = 60;   // угол между кнопками подменю, °
    public const double Hyst = 25;        // гистерезис выхода с подпункта
    public const double HoverGrow = 0.08;
    public const double AppearT = 0.24;
    public const double FadeIn = 0.14;
    public const double FadeOut = 0.12;
    public const double ColorT = 0.07;
    public const int TickMs = 8;

    /// <summary>Пересчитать размеры: базовые × размер из настроек, и вписать в экран, если он маленький.</summary>
    public static void Recompute(int sw, int sh)
    {
        double s = S.SizePercent / 100.0;
        MainD = 117 * s; FanD = 116 * s; ROut = 156 * s; RMax = 168 * s; RMaxSub = 246 * s;
        Dead = 34 * s; CircleR = 32 * s; HubR = 17 * s;
        double k = Math.Min(1.0, (Math.Min(sw, sh) / 2.0 - 24) / (MainD + FanD + CircleR * 1.2 + 24));
        MainD *= k; FanD *= k; ROut *= k; RMax *= k; RMaxSub *= k; Dead *= k; CircleR *= k; HubR *= k;
        Sens = 0.5 * S.SensPercent / 100.0;
        DwellT = S.DwellMs / 1000.0;
    }
}
