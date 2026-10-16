using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Drawing.Text;
using System.Reflection;
using System.Runtime.InteropServices;

namespace RadialMenu;

/// <summary>
/// «Стеклянные» кнопки в стиле Actions Ring. Кнопка = круг диаметром 64 в квадрате 80×80:
/// мягкая тень только снаружи круга, полупрозрачная заливка, обводка 1px, иконка.
/// Спрайт хранится как premultiplied BGRA; рисуется аналитически, без GDI.
/// </summary>
internal static class Sprites
{
    public const int Box = 80;
    public const double Diam = 64;
    public const int DotBox = 16;
    public const int IconPx = 24;

    public static readonly string[] StateNames = { "default", "open", "hover", "on" };

    // заливка, прозрачность, обводка, иконка
    static readonly (double[] fill, double op, double[] stroke, double[] icon)[] Styles =
    {
        (new double[] { 204, 204, 204 }, 0.70, new double[] { 255, 255, 255 }, new double[] { 16, 16, 16 }),     // default
        (new double[] { 77, 94, 128 },  0.75, new double[] { 179, 204, 255 }, new double[] { 255, 255, 255 }),  // open
        (new double[] { 105, 155, 255 }, 0.80, new double[] { 179, 204, 255 }, new double[] { 255, 255, 255 }), // hover
        (new double[] { 77, 128, 94 },  0.75, new double[] { 179, 255, 204 }, new double[] { 255, 255, 255 }),  // on
    };

    static readonly Dictionary<(int, string), byte[]> Cache = new();
    static readonly Dictionary<string, float[]> Masks = new();
    static byte[] _dot;

    public static byte[] Get(int state, string icon)
    {
        var key = (state, icon);
        if (!Cache.TryGetValue(key, out var s))
            Cache[key] = s = Build(state, icon);
        return s;
    }

    static double Cov(double d, double r) => Math.Clamp(r - d + 0.5, 0, 1);

    static double Erfc(double x)
    {
        // Abramowitz-Stegun 7.1.26
        double z = Math.Abs(x), t = 1 / (1 + 0.3275911 * z);
        double y = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429)))) * Math.Exp(-z * z);
        return x >= 0 ? y : 2 - y;
    }

    static byte[] Build(int state, string icon)
    {
        var st = Styles[state];
        var mask = Mask(icon);
        var px = new byte[Box * Box * 4];
        double c = Box / 2.0, R = Diam / 2;
        int io = (Box - IconPx) / 2;
        for (int y = 0; y < Box; y++)
            for (int x = 0; x < Box; x++)
            {
                double d = Math.Sqrt((x + 0.5 - c) * (x + 0.5 - c) + (y + 0.5 - c) * (y + 0.5 - c));
                double cov = Cov(d, R);
                // тень: гауссово размытие круга (σ=4), 30% чёрного, только снаружи
                double sh = 0.30 * 0.5 * Erfc((d - R) / (4 * Math.Sqrt(2))) * (1 - cov);
                double ra = sh, rr = 0, rg = 0, rb = 0;   // premultiplied
                // заливка
                double fa = cov * st.op;
                Over(ref rr, ref rg, ref rb, ref ra, st.fill[0] * fa, st.fill[1] * fa, st.fill[2] * fa, fa);
                // обводка 1px внутри края
                double sa = Math.Max(0, cov - Cov(d, R - 1));
                Over(ref rr, ref rg, ref rb, ref ra, st.stroke[0] * sa, st.stroke[1] * sa, st.stroke[2] * sa, sa);
                // иконка
                int ix = x - io, iy = y - io;
                if (ix >= 0 && iy >= 0 && ix < IconPx && iy < IconPx)
                {
                    double ia = mask[iy * IconPx + ix];
                    if (ia > 0) Over(ref rr, ref rg, ref rb, ref ra, st.icon[0] * ia, st.icon[1] * ia, st.icon[2] * ia, ia);
                }
                int o = (y * Box + x) * 4;
                px[o] = (byte)Math.Round(Math.Clamp(rb, 0, 255));
                px[o + 1] = (byte)Math.Round(Math.Clamp(rg, 0, 255));
                px[o + 2] = (byte)Math.Round(Math.Clamp(rr, 0, 255));
                px[o + 3] = (byte)Math.Round(Math.Clamp(ra * 255, 0, 255));
            }
        // rr/rg/rb накапливались в диапазоне 0..255 * альфа(0..1) - уже premultiplied
        return px;
    }

    /// <summary>top over bottom; цвета premultiplied (0..255 * a), альфа 0..1.</summary>
    static void Over(ref double r, ref double g, ref double b, ref double a, double tr, double tg, double tb, double ta)
    {
        double k = 1 - ta;
        r = tr + r * k; g = tg + g * k; b = tb + b * k; a = ta + a * k;
    }

    public static byte[] Dot()
    {
        if (_dot != null) return _dot;
        var px = new byte[DotBox * DotBox * 4];
        double c = DotBox / 2.0, R = DotBox / 2.0 - 2;
        for (int y = 0; y < DotBox; y++)
            for (int x = 0; x < DotBox; x++)
            {
                double d = Math.Sqrt((x + 0.5 - c) * (x + 0.5 - c) + (y + 0.5 - c) * (y + 0.5 - c));
                double cov = Cov(d, R);
                double edge = Math.Max(0, cov - Cov(d, R - 0.8));
                double r = 76 * cov, g = 125 * cov, b = 255 * cov, a = cov;
                double k = 1 - edge;
                r = 179 * edge + r * k; g = 204 * edge + g * k; b = 255 * edge + b * k; a = edge + a * k;
                int o = (y * DotBox + x) * 4;
                px[o] = (byte)Math.Round(b); px[o + 1] = (byte)Math.Round(g); px[o + 2] = (byte)Math.Round(r);
                px[o + 3] = (byte)Math.Round(a * 255);
            }
        return _dot = px;
    }

    /// <summary>Альфа-маска иконки 24×24: берётся из встроенного PNG 96×96 (серый = прозрачность), усредняется 4×4.</summary>
    static float[] Mask(string key)
    {
        if (Masks.TryGetValue(key, out var m)) return m;
        m = new float[IconPx * IconPx];
        if (key != "blank")
        {
            var asm = Assembly.GetExecutingAssembly();
            string name = asm.GetManifestResourceNames().FirstOrDefault(n => n.EndsWith("." + key + ".png", StringComparison.Ordinal));
            if (name != null)
            {
                using var bmp = new Bitmap(asm.GetManifestResourceStream(name));
                int k = bmp.Width / IconPx;
                var data = bmp.LockBits(new Rectangle(0, 0, bmp.Width, bmp.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
                try
                {
                    var buf = new byte[data.Stride * bmp.Height];
                    Marshal.Copy(data.Scan0, buf, 0, buf.Length);
                    for (int y = 0; y < IconPx; y++)
                        for (int x = 0; x < IconPx; x++)
                        {
                            double sum = 0;
                            for (int j = 0; j < k; j++)
                                for (int i = 0; i < k; i++)
                                    sum += buf[(y * k + j) * data.Stride + (x * k + i) * 4 + 2];  // R-канал серой маски
                            m[y * IconPx + x] = (float)(sum / (k * k) / 255.0);
                        }
                }
                finally { bmp.UnlockBits(data); }
            }
        }
        Masks[key] = m;
        return m;
    }

    // ---------------------------------------------------------------- подписи
    static readonly Dictionary<(string, int), Bitmap> Pills = new();

    /// <summary>Плашка с подписью (тёмная полупрозрачная, белый текст), 32bppPArgb.</summary>
    public static Bitmap Pill(string text, int size = 13)
    {
        if (Pills.TryGetValue((text, size), out var cached)) return cached;
        using var font = new Font("Segoe UI", size, FontStyle.Bold, GraphicsUnit.Pixel);
        SizeF ts;
        using (var tmp = new Bitmap(1, 1))
        using (var gt = Graphics.FromImage(tmp))
            ts = gt.MeasureString(text, font, 2000, StringFormat.GenericTypographic);
        int w = (int)Math.Ceiling(ts.Width) + 18, h = (int)Math.Ceiling(ts.Height) + 8;
        var bmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.TextRenderingHint = TextRenderingHint.AntiAlias;
            using var path = RoundRect(new RectangleF(0.5f, 0.5f, w - 1, h - 1), (h - 1) / 2f);
            using (var br = new SolidBrush(Color.FromArgb(185, 16, 16, 16))) g.FillPath(br, path);
            using (var pen = new Pen(Color.FromArgb(70, 255, 255, 255), 1)) g.DrawPath(pen, path);
            g.DrawString(text, font, Brushes.White, 9, 4, StringFormat.GenericTypographic);
        }
        if (Pills.Count > 64) { foreach (var b in Pills.Values) b.Dispose(); Pills.Clear(); }
        return Pills[(text, size)] = bmp;
    }

    static GraphicsPath RoundRect(RectangleF r, float rad)
    {
        float d = rad * 2;
        var p = new GraphicsPath();
        p.AddArc(r.X, r.Y, d, d, 180, 90);
        p.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        p.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        p.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        p.CloseFigure();
        return p;
    }
}
