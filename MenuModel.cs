namespace RadialMenu;

internal sealed class Node
{
    public string Id, Icon, Label;
    public Action Action;
    public bool Dwell;                  // подменю раскрывается только после задержки (сама кнопка тоже выполняется)
    public Func<(string icon, string label)> Dyn;   // иконка/подпись, зависящие от ситуации
    public Func<bool> State;            // блок «включён» (зелёный стиль)
    public List<Node> Children = new();
    public Node Parent;

    public double P;                    // появление 0..1 (отрицательное = задержка)
    public double Hv;                   // наведение 0..1
    public double[] Sw;                 // веса состояний default/open/hover/on
    public double OffX, OffY;           // положение относительно центра меню
    public double A1, A2;               // угловой сектор выбора, ° (y вниз)

    public Node(string icon, string label, Action action = null, bool dwell = false,
                Func<(string, string)> dyn = null, Func<bool> state = null, params Node[] children)
    {
        Icon = icon; Label = label; Action = action; Dwell = dwell; Dyn = dyn; State = state;
        Children.AddRange(children);
        foreach (var c in Children) c.Parent = this;
    }

    public double MidAngle => (A1 + A2) / 2;
}

internal static class MenuModel
{
    public static readonly MediaState Media = new();
    public static Node Root;

    static Node Id(string id, Node n) { n.Id = id; return n; }

    /// <summary>Полное дерево меню (с id для настроек).</summary>
    public static Node FullTree() => new Node("", "", null, false, null, null,
        Id("open", new Node("square-plus", "Открыть", null, false, null, null,
            Id("open.explorer", new Node("folder", "Проводник", Actions.Explorer)),
            Id("open.settings", new Node("gear", "Параметры", Actions.Settings)),
            Id("open.note", new Node("pencil-to-square", "Заметка", Actions.Note)))),
        Id("paste", new Node("paste", "Вставить", () => Actions.SendCtrl(0x56), true, null, null,
            Id("paste.clipboard", new Node("clock-arrow-rotate-left", "Буфер обмена", Actions.WinV)))),
        Id("media", new Node("headphones", "Медиа", null, false, null, () => Media.Playing,
            Id("media.prev", new Node("backward-step", "Назад", () => Actions.MediaKey(0xB1))),
            Id("media.play", new Node("pause", "Пауза", () => Actions.MediaKey(0xB3), false,
                () => Media.Playing ? ("pause", "Пауза") : ("play", "Плей"))),
            Id("media.next", new Node("forward-step", "Вперёд", () => Actions.MediaKey(0xB0))))),
        Id("copy", new Node("copy", "Копировать", () => Actions.SendCtrl(0x43), true, null, null,
            Id("copy.selectall", new Node("square-dashed", "Выбрать все", () => Actions.SendCtrl(0x41))),
            Id("copy.cut", new Node("scissors", "Вырезать", () => Actions.SendCtrl(0x58))))));

    /// <summary>Собрать меню без отключённых пунктов. Группа без действия и без подпунктов исчезает целиком.</summary>
    public static void Build()
    {
        var disabled = new HashSet<string>(Cfg.S.DisabledItems);
        var root = FullTree();
        foreach (var g in root.Children.ToList())
        {
            if (disabled.Contains(g.Id)) { root.Children.Remove(g); continue; }
            foreach (var c in g.Children.ToList())
                if (disabled.Contains(c.Id)) g.Children.Remove(c);
            if (g.Children.Count == 0 && g.Action == null) root.Children.Remove(g);
        }
        if (root.Children.Count == 0) root = FullTree();   // защита: пустое меню не имеет смысла
        Root = root;
        LayoutRoot();
    }

    public static IEnumerable<Node> All()
    {
        foreach (var c in Root.Children)
        {
            yield return c;
            foreach (var g in c.Children) yield return g;
        }
    }

    /// <summary>Основные кнопки на расстоянии MainD: сверху, справа, снизу, слева (по часовой).</summary>
    public static void LayoutRoot()
    {
        int n = Root.Children.Count;
        double span = 360.0 / n;
        for (int i = 0; i < n; i++)
        {
            var k = Root.Children[i];
            double mid = 270 + i * span;
            k.A1 = mid - span / 2 + 1.5;
            k.A2 = mid + span / 2 - 1.5;
            k.OffX = Cfg.MainD * Math.Cos(Rad(mid));
            k.OffY = Cfg.MainD * Math.Sin(Rad(mid));
        }
    }

    /// <summary>Подменю веером наружу от основной кнопки: FanD от неё, шаг FanSpread°.</summary>
    public static void LayoutSub(Node node)
    {
        double mid = node.MidAngle;
        int n = node.Children.Count;
        (double x, double y) Spot(double deg)
        {
            double a = Rad(mid + deg);
            return (node.OffX + Cfg.FanD * Math.Cos(a), node.OffY + Cfg.FanD * Math.Sin(a));
        }
        bool vertical = Math.Abs(Math.Sin(Rad(mid))) > Math.Abs(Math.Cos(Rad(mid)));
        var p = Spot(Cfg.FanSpread); var m = Spot(-Cfg.FanSpread);
        // порядок кнопок: слева направо (если веер вертикальный, т.е. сверху/снизу) или сверху вниз (если сбоку)
        double hi = vertical ? p.x : p.y, lo = vertical ? m.x : m.y;
        int sign = hi > lo ? 1 : -1;
        for (int j = 0; j < n; j++)
        {
            var k = node.Children[j];
            var s = Spot((j - (n - 1) / 2.0) * Cfg.FanSpread * sign);
            k.OffX = s.x; k.OffY = s.y;
            double ang = (Math.Atan2(k.OffY, k.OffX) * 180 / Math.PI + 360) % 360;
            double half = Math.Min(Cfg.FanSpread, 90.0 / n) / 2;
            k.A1 = ang - half + 1;
            k.A2 = ang + half - 1;
        }
    }

    public static double Rad(double deg) => deg * Math.PI / 180;

    public static double AngDiff(double a, double b)
    {
        double d = ((a - b + 180) % 360 + 360) % 360 - 180;
        return Math.Abs(d);
    }
}
