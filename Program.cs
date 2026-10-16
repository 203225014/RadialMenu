using System.Drawing;
using System.Reflection;
using System.Windows.Forms;
using static RadialMenu.Native;

namespace RadialMenu;

internal static class Program
{
    static Overlay _overlay;
    static InputHook _hook;

    [STAThread]
    static void Main(string[] args)
    {
        // один экземпляр: два хука на одну кнопку мешают друг другу
        using var mutex = new Mutex(true, @"Local\RadialMenu_Singleton", out bool first);
        const string ShowEventName = @"Local\RadialMenu_ShowSettings";
        if (!first)
        {
            // повторный запуск (ярлык, меню Пуск): попросить работающий экземпляр открыть настройки
            try { EventWaitHandle.OpenExisting(ShowEventName).Set(); } catch { }
            return;
        }
        var showEvent = new EventWaitHandle(false, EventResetMode.AutoReset, ShowEventName);

        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.EnableVisualStyles();
        AppDomain.CurrentDomain.UnhandledException += (_, e) => Log.Error("необработанная", e.ExceptionObject as Exception ?? new Exception(e.ExceptionObject?.ToString()));
        TaskScheduler.UnobservedTaskException += (_, e) => Log.Error("task", e.Exception);
        Application.ThreadException += (_, e) => Log.Error("ui", e.Exception);

        Cfg.S = Settings.Load();
        // параметры командной строки действуют только на этот запуск
        for (int i = 0; i < args.Length - 1; i++)
        {
            if (args[i] == "--button" && int.TryParse(args[i + 1], out int b) && b is 1 or 2) Cfg.S.Button = b;
            if (args[i] == "--vault") Cfg.S.Vault = args[i + 1];
        }

        // системные курсоры прошлых версий/сбоев: на всякий случай вернуть стандартные
        SystemParametersInfoW(0x57, 0, 0, 0);

        // скрытый элемент главного потока: через него фоновые потоки открывают окна
        var ui = new Control();
        ui.CreateControl();
        _ = ui.Handle;
        NoteWindow.Ui = ui;

        _hook = new InputHook(Cfg.Button);
        _overlay = new Overlay(_hook);
        _hook.Start();
        _overlay.Start();

        using var tray = new NotifyIcon
        {
            Icon = LoadIcon(),
            Text = "Радиальное меню " + Cfg.Version,
            Visible = true,
            ContextMenuStrip = new ContextMenuStrip(),
        };
        var menu = tray.ContextMenuStrip;
        menu.Items.Add("Настройки…", null, (_, _) => OpenSettings());
        var pause = new ToolStripMenuItem("Пауза");
        pause.Click += (_, _) =>
        {
            _hook.Enabled = !_hook.Enabled;
            pause.Text = _hook.Enabled ? "Пауза" : "Продолжить";
            tray.Text = "Радиальное меню " + Cfg.Version + (_hook.Enabled ? "" : " (пауза)");
        };
        menu.Items.Add(pause);
        menu.Items.Add("Проверить обновления", null, async (_, _) => await UpdateChecker.CheckAsync(tray, true));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Выход", null, (_, _) => { _overlay.Stop(); Application.Exit(); });
        tray.DoubleClick += (_, _) => OpenSettings();
        new Thread(() =>
        {
            while (showEvent.WaitOne()) ui.BeginInvoke(new Action(OpenSettings));
        }) { IsBackground = true }.Start();
        if (args.Contains("--settings")) ui.BeginInvoke(new Action(OpenSettings));

        // проверка при запуске и затем раз в сутки
        var daily = new System.Windows.Forms.Timer { Interval = 24 * 60 * 60 * 1000 };
        daily.Tick += async (_, _) => await UpdateChecker.CheckAsync(tray, false);
        daily.Start();
        _ = Task.Delay(15000).ContinueWith(_ => ui.BeginInvoke(new Action(async () => await UpdateChecker.CheckAsync(tray, false))));

        Application.Run();
        tray.Visible = false;
    }

    static void OpenSettings() => SettingsForm.OpenOrFocus(ApplySettings);

    static void ApplySettings(Settings s, bool autostart)
    {
        Cfg.S = s;
        s.Save();
        Autostart.Set(autostart);
        _overlay.Rebuild();   // размер, пункты, кнопка применятся, когда меню не на экране
    }

    static Icon LoadIcon()
    {
        try
        {
            var asm = Assembly.GetExecutingAssembly();
            string name = asm.GetManifestResourceNames().First(n => n.EndsWith(".RadialMenu.ico", StringComparison.Ordinal));
            return new Icon(asm.GetManifestResourceStream(name));
        }
        catch { return SystemIcons.Application; }
    }
}
