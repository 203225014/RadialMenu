using System.Diagnostics;
using System.Net.Http;
using System.Text.Json;
using System.Windows.Forms;

namespace RadialMenu;

/// <summary>Проверка новой версии: сравнивает свою версию с последним релизом на GitHub.</summary>
internal static class UpdateChecker
{
    const string Repo = "203225014/RadialMenu";
    static string _url = $"https://github.com/{Repo}/releases/latest";

    /// <summary>Спросить GitHub; если есть версия новее, показать уведомление (клик открывает страницу релиза).</summary>
    public static async Task CheckAsync(NotifyIcon tray, bool manual)
    {
        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(10) };
            http.DefaultRequestHeaders.UserAgent.ParseAdd("RadialMenu/" + Cfg.Version);
            string json = await http.GetStringAsync($"https://api.github.com/repos/{Repo}/releases/latest");
            using var doc = JsonDocument.Parse(json);
            string tag = doc.RootElement.GetProperty("tag_name").GetString() ?? "";
            if (doc.RootElement.TryGetProperty("html_url", out var u) && u.GetString() is { } url) _url = url;

            if (Version.TryParse(tag.TrimStart('v', 'V'), out var latest) && Version.TryParse(Cfg.Version, out var cur) && latest > cur)
            {
                tray.BalloonTipClicked -= OnClick;
                tray.BalloonTipClicked += OnClick;
                tray.ShowBalloonTip(10000, "Доступна новая версия " + latest, "Установлена " + cur + ". Нажмите, чтобы открыть страницу загрузки.", ToolTipIcon.Info);
            }
            else if (manual)
                tray.ShowBalloonTip(4000, "Радиальное меню", "У вас последняя версия (" + Cfg.Version + ").", ToolTipIcon.Info);
        }
        catch (Exception e)
        {
            // нет интернета или репозиторий без релизов: молча, кроме ручной проверки
            if (manual) tray.ShowBalloonTip(4000, "Радиальное меню", "Не удалось проверить обновления.", ToolTipIcon.Warning);
            else Log.Error("проверка обновлений", e);
        }
    }

    static void OnClick(object s, EventArgs e)
    {
        try { Process.Start(new ProcessStartInfo(_url) { UseShellExecute = true }); } catch { }
    }
}
