"""Отрисовка «стеклянных» кнопок (стиль Actions Ring) и полупрозрачное окно по пикселям.

Стили состояний взяты из макетов default.svg / intermediate.svg / hover.svg:
  default       заливка #CCCCCC 70%, обводка белая,   иконка #101010
  open          заливка #4D5E80 75%, обводка #B3CCFF, иконка белая   (intermediate: блок с открытым подменю)
  hover         заливка #699BFF 80%, обводка #B3CCFF, иконка белая
  on            заливка #4D805E 75%, обводка #B3FFCC, иконка белая   (своё расширение: «Медиа» играет)
Кнопка: круг диаметром 64 в квадрате 80×80, мягкая тень (blur 4, чёрная 30%) только снаружи круга.
"""
import ctypes
import math
import os
import re
from ctypes import wintypes

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
ICON_DIR = os.path.join(HERE, "icons")

BOX = 80          # размер спрайта, px
DIAM = 64         # диаметр круга в спрайте
SS = 4            # суперсэмплинг при построении

STATES = {
    "default": dict(fill=(204, 204, 204), op=0.70, stroke=(255, 255, 255), icon=(16, 16, 16)),
    "open": dict(fill=(77, 94, 128), op=0.75, stroke=(179, 204, 255), icon=(255, 255, 255)),
    "hover": dict(fill=(105, 155, 255), op=0.80, stroke=(179, 204, 255), icon=(255, 255, 255)),
    "on": dict(fill=(77, 128, 94), op=0.75, stroke=(179, 255, 204), icon=(255, 255, 255)),
}

FONT_PATHS = (r"C:\Windows\Fonts\segoeuib.ttf", r"C:\Windows\Fonts\arialbd.ttf")


def _premul_resize(img, size, resample=Image.LANCZOS):
    return img.convert("RGBa").resize(size, resample).convert("RGBA")


def _circle_mask(inset=0.0):
    m = Image.new("L", (BOX * SS, BOX * SS), 0)
    a = (BOX - DIAM) / 2 + inset
    d = ImageDraw.Draw(m)
    d.ellipse([a * SS, a * SS, (BOX - a) * SS - 1, (BOX - a) * SS - 1], fill=255)
    return m


def _glass_base(state):
    """Тень + заливка + обводка без иконки, 80×80 RGBA."""
    st = STATES[state]
    S = BOX * SS
    m = _circle_mask()
    # тень: blur σ=4 от круга, 30% чёрного, вычитая сам круг (только снаружи)
    sh = m.filter(ImageFilter.GaussianBlur(4 * SS)).point(lambda v: int(v * 0.30))
    sh = Image.composite(Image.new("L", (S, S), 0), sh, m)
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    layer.putalpha(sh)
    # заливка с прозрачностью
    fill = Image.new("RGBA", (S, S), st["fill"] + (0,))
    fill.putalpha(m.point(lambda v: int(v * st["op"])))
    layer.alpha_composite(fill)
    # обводка 1px внутри края
    ring = Image.composite(Image.new("L", (S, S), 0), m, _circle_mask(1.0))
    stroke = Image.new("RGBA", (S, S), st["stroke"] + (0,))
    stroke.putalpha(ring)
    layer.alpha_composite(stroke)
    return _premul_resize(layer, (BOX, BOX))


def _draw_pause():
    k = 8
    im = Image.new("L", (24 * k, 24 * k), 0)
    d = ImageDraw.Draw(im)
    for x0 in (5.2, 13.0):
        d.rounded_rectangle([x0 * k, 4 * k, (x0 + 5.8) * k, 20 * k], radius=1.6 * k, outline=255, width=int(2.0 * k))
    return im.resize((24, 24), Image.LANCZOS)


def _draw_cross():
    k = 8
    im = Image.new("L", (24 * k, 24 * k), 0)
    d = ImageDraw.Draw(im)
    w = int(2.2 * k)
    d.line([7 * k, 7 * k, 17 * k, 17 * k], fill=255, width=w)
    d.line([17 * k, 7 * k, 7 * k, 17 * k], fill=255, width=w)
    for x, y in ((7, 7), (17, 17), (17, 7), (7, 17)):
        d.ellipse([(x - 1.1) * k, (y - 1.1) * k, (x + 1.1) * k, (y + 1.1) * k], fill=255)
    return im.resize((24, 24), Image.LANCZOS)


# ---------------------------------------------------------------- SVG-иконки (Gravity UI)
_NUM = re.compile(r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
_ARGS = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}


def _arc_points(x1, y1, rx, ry, phi, fa, fs, x2, y2, steps=24):
    """Дуга SVG (endpoint -> center parametrization, W3C F.6.5) -> точки."""
    if rx == 0 or ry == 0 or (x1 == x2 and y1 == y2):
        return [(x2, y2)]
    rx, ry = abs(rx), abs(ry)
    cp, sp = math.cos(math.radians(phi)), math.sin(math.radians(phi))
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p, y1p = cp * dx + sp * dy, -sp * dx + cp * dy
    lam = x1p ** 2 / rx ** 2 + y1p ** 2 / ry ** 2
    if lam > 1:
        rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
    num = rx ** 2 * ry ** 2 - rx ** 2 * y1p ** 2 - ry ** 2 * x1p ** 2
    den = rx ** 2 * y1p ** 2 + ry ** 2 * x1p ** 2
    co = math.sqrt(max(0.0, num / den)) * (-1 if fa == fs else 1)
    cxp, cyp = co * rx * y1p / ry, -co * ry * x1p / rx
    cx, cy = cp * cxp - sp * cyp + (x1 + x2) / 2, sp * cxp + cp * cyp + (y1 + y2) / 2

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not fs and dt > 0:
        dt -= 2 * math.pi
    elif fs and dt < 0:
        dt += 2 * math.pi
    pts = []
    for i in range(1, steps + 1):
        t = t1 + dt * i / steps
        ex, ey = rx * math.cos(t), ry * math.sin(t)
        pts.append((cp * ex - sp * ey + cx, sp * ex + cp * ey + cy))
    return pts


def _flatten_path(d):
    """Строка d -> список подконтуров (списки точек)."""
    i, n = 0, len(d)
    subs, cur = [], []
    x = y = sx = sy = 0.0
    cmd = None
    prev_c = prev_q = None  # для S / T

    def skip():
        nonlocal i
        while i < n and d[i] in " ,\t\r\n":
            i += 1

    def num():
        nonlocal i
        skip()
        m = _NUM.match(d, i)
        i = m.end()
        return float(m.group())

    def flag():
        nonlocal i
        skip()
        ch = d[i]
        i += 1
        return int(ch)

    def bez(p0, p1, p2, p3, steps=14):
        out = []
        for k in range(1, steps + 1):
            t = k / steps
            u = 1 - t
            out.append((u ** 3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
                        u ** 3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1]))
        return out

    while True:
        skip()
        if i >= n:
            break
        if d[i].isalpha():
            cmd = d[i]
            i += 1
            if cmd in "Zz":
                if cur:
                    subs.append(cur)
                cur = []
                x, y = sx, sy
                prev_c = prev_q = None
                continue
        elif cmd is None:
            break
        up = cmd.upper()
        rel = cmd.islower()
        if up == "M":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            if cur:
                subs.append(cur)
            cur = [(nx, ny)]
            x, y = sx, sy = nx, ny
            cmd = "l" if rel else "L"  # следующие пары - линии
            prev_c = prev_q = None
        elif up == "L":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            cur.append((nx, ny))
            x, y = nx, ny
            prev_c = prev_q = None
        elif up == "H":
            nx = num()
            x = x + nx if rel else nx
            cur.append((x, y))
            prev_c = prev_q = None
        elif up == "V":
            ny = num()
            y = y + ny if rel else ny
            cur.append((x, y))
            prev_c = prev_q = None
        elif up == "C":
            a = [num() for _ in range(6)]
            if rel:
                a = [a[0] + x, a[1] + y, a[2] + x, a[3] + y, a[4] + x, a[5] + y]
            cur += bez((x, y), (a[0], a[1]), (a[2], a[3]), (a[4], a[5]))
            prev_c = (a[2], a[3])
            prev_q = None
            x, y = a[4], a[5]
        elif up == "S":
            a = [num() for _ in range(4)]
            if rel:
                a = [a[0] + x, a[1] + y, a[2] + x, a[3] + y]
            c1 = (2 * x - prev_c[0], 2 * y - prev_c[1]) if prev_c else (x, y)
            cur += bez((x, y), c1, (a[0], a[1]), (a[2], a[3]))
            prev_c = (a[0], a[1])
            prev_q = None
            x, y = a[2], a[3]
        elif up in "QT":
            if up == "Q":
                a = [num() for _ in range(4)]
                if rel:
                    a = [a[0] + x, a[1] + y, a[2] + x, a[3] + y]
                q1 = (a[0], a[1])
                ex, ey = a[2], a[3]
            else:
                ex, ey = num(), num()
                if rel:
                    ex, ey = x + ex, y + ey
                q1 = (2 * x - prev_q[0], 2 * y - prev_q[1]) if prev_q else (x, y)
            c1 = (x + 2 / 3 * (q1[0] - x), y + 2 / 3 * (q1[1] - y))
            c2 = (ex + 2 / 3 * (q1[0] - ex), ey + 2 / 3 * (q1[1] - ey))
            cur += bez((x, y), c1, c2, (ex, ey))
            prev_q = q1
            prev_c = None
            x, y = ex, ey
        elif up == "A":
            rx, ry, phi = num(), num(), num()
            fa, fs = flag(), flag()
            ex, ey = num(), num()
            if rel:
                ex, ey = x + ex, y + ey
            cur += _arc_points(x, y, rx, ry, phi, fa, fs, ex, ey)
            x, y = ex, ey
            prev_c = prev_q = None
        else:
            break
    if cur:
        subs.append(cur)
    return subs


def _area(poly):
    return sum(poly[k][0] * poly[(k + 1) % len(poly)][1] - poly[(k + 1) % len(poly)][0] * poly[k][1]
               for k in range(len(poly))) / 2


def svg_mask(path, size=24, glyph=24.0, ss=8):
    """Альфа-маска size×size из SVG Gravity UI (viewBox 16): глyph px по центру, заливка nonzero/evenodd."""
    text = open(path, encoding="utf-8").read()
    vb = re.search(r'viewBox="([\d.\s-]+)"', text)
    vbw = float(vb.group(1).split()[2]) if vb else 16.0
    k = glyph / vbw
    off = (size - glyph) / 2
    W = size * ss
    result = Image.new("L", (W, W), 0)
    for tag in re.findall(r"<path\b[^>]*>", text):
        d = re.search(r'\bd="([^"]+)"', tag)
        if not d:
            continue
        evenodd = "evenodd" in tag
        polys = [[((px * k + off) * ss, (py * k + off) * ss) for px, py in sub]
                 for sub in _flatten_path(d.group(1)) if len(sub) >= 3]
        if not polys:
            continue
        masks = []
        for poly in polys:
            m = Image.new("L", (W, W), 0)
            ImageDraw.Draw(m).polygon(poly, fill=255)
            masks.append((_area(poly), m))
        layer = Image.new("L", (W, W), 0)
        if evenodd:
            for _, m in masks:
                layer = ImageChops.difference(layer, m)  # XOR для бинарных масок
        else:
            main = max(masks, key=lambda t: abs(t[0]))[0] > 0
            same = Image.new("L", (W, W), 0)
            opposite = Image.new("L", (W, W), 0)
            for a, m in masks:
                if (a > 0) == main:
                    same = ImageChops.lighter(same, m)
                else:
                    opposite = ImageChops.lighter(opposite, m)
            layer = ImageChops.subtract(same, opposite)
        result = ImageChops.lighter(result, layer)
    return result.resize((size, size), Image.LANCZOS)


_icon_masks = {}


def icon_mask(key):
    """Альфа-маска иконки 24×24 (из PNG либо нарисованная)."""
    if key not in _icon_masks:
        svg = os.path.join(ICON_DIR, key + ".svg")
        if os.path.exists(svg):
            _icon_masks[key] = svg_mask(svg)  # Gravity UI
        elif key == "pause":
            _icon_masks[key] = _draw_pause()
        elif key == "blank":
            _icon_masks[key] = Image.new("L", (24, 24), 0)
        elif key == "cross":
            _icon_masks[key] = _draw_cross()
        else:
            im = Image.open(os.path.join(ICON_DIR, key + ".png")).convert("RGBA")
            _icon_masks[key] = im.getchannel("A")
    return _icon_masks[key]


_sprites = {}
DOT_BOX = 16      # размер синей точки в центре, px


def dot_sprite():
    """Синяя точка (указатель мыши) в центре меню."""
    k = ("dot", 0)
    if k not in _sprites:
        S = 8
        im = Image.new("RGBA", (DOT_BOX * S, DOT_BOX * S), (0, 0, 0, 0))
        d = ImageDraw.Draw(im)
        m = 2 * S
        d.ellipse([m, m, DOT_BOX * S - m, DOT_BOX * S - m], fill=(76, 125, 255, 255),
                  outline=(179, 204, 255, 255), width=int(0.8 * S))
        _sprites[k] = _premul_resize(im, (DOT_BOX, DOT_BOX))
    return _sprites[k]



def sprite(state, icon_key):
    """Готовая кнопка 80×80 RGBA в заданном состоянии с иконкой."""
    k = (state, icon_key)
    if k not in _sprites:
        img = _glass_base(state).copy()
        col = STATES[state]["icon"]
        ic = Image.new("RGBA", (24, 24), col + (0,))
        ic.putalpha(icon_mask(icon_key))
        img.alpha_composite(ic, ((BOX - 24) // 2, (BOX - 24) // 2))
        _sprites[k] = img
    return _sprites[k]


_fonts = {}


def font(size):
    if size not in _fonts:
        for p in FONT_PATHS:
            try:
                _fonts[size] = ImageFont.truetype(p, size)
                break
            except OSError:
                continue
        else:
            _fonts[size] = ImageFont.load_default()
    return _fonts[size]


def label_pill(text, alpha=1.0, size=13):
    """Плашка с подписью (тёмная полупрозрачная, белый текст)."""
    f = font(size)
    l, t, r, b = f.getbbox(text)
    w, h = r - l + 18, (b - t) + 12
    im = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([0, 0, w - 1, h - 1], radius=h // 2, fill=(16, 16, 16, int(185 * alpha)),
                        outline=(255, 255, 255, int(70 * alpha)))
    d.text((9 - l, 6 - t), text, font=f, fill=(255, 255, 255, int(255 * alpha)))
    return im


# ---------------------------------------------------------------- окно по пикселям
user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
kernel32 = ctypes.windll.kernel32


class _BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD),
                ("biClrImportant", wintypes.DWORD)]


class _BLEND(ctypes.Structure):
    _fields_ = [("BlendOp", ctypes.c_ubyte), ("BlendFlags", ctypes.c_ubyte),
                ("SourceConstantAlpha", ctypes.c_ubyte), ("AlphaFormat", ctypes.c_ubyte)]


class Layer:
    """Полупрозрачное окно поверх всех, не забирающее фокус и пропускающее клики."""
    WS_POPUP = 0x80000000
    EX = 0x00080000 | 0x00000080 | 0x00000008 | 0x08000000 | 0x00000020  # LAYERED TOOLWINDOW TOPMOST NOACTIVATE TRANSPARENT

    def __init__(self, x, y, w, h):
        self.x, self.y, self.w, self.h = int(x), int(y), int(w), int(h)
        user32.CreateWindowExW.restype = wintypes.HWND
        user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                           ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                           wintypes.HWND, wintypes.HANDLE, wintypes.HINSTANCE, ctypes.c_void_p]
        kernel32.GetModuleHandleW.restype = wintypes.HMODULE
        kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
        self.hwnd = user32.CreateWindowExW(self.EX, "Static", "RadialMenuLayer", self.WS_POPUP,
                                           self.x, self.y, self.w, self.h, None, None,
                                           kernel32.GetModuleHandleW(None), None)
        if not self.hwnd:
            raise OSError("CreateWindowEx failed")
        user32.GetDC.restype = wintypes.HDC
        user32.GetDC.argtypes = [wintypes.HWND]
        self.hdc_screen = user32.GetDC(None)
        gdi32.CreateCompatibleDC.restype = wintypes.HDC
        gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
        self.hdc = gdi32.CreateCompatibleDC(self.hdc_screen)
        bmi = _BITMAPINFOHEADER(ctypes.sizeof(_BITMAPINFOHEADER), self.w, -self.h, 1, 32, 0, 0, 0, 0, 0, 0)
        self.bits = ctypes.c_void_p()
        gdi32.CreateDIBSection.restype = wintypes.HBITMAP
        gdi32.CreateDIBSection.argtypes = [wintypes.HDC, ctypes.c_void_p, wintypes.UINT,
                                           ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.DWORD]
        self.hbmp = gdi32.CreateDIBSection(self.hdc, ctypes.byref(bmi), 0, ctypes.byref(self.bits), None, 0)
        gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HANDLE]
        gdi32.SelectObject(self.hdc, self.hbmp)
        user32.UpdateLayeredWindow.argtypes = [wintypes.HWND, wintypes.HDC, ctypes.c_void_p, ctypes.c_void_p,
                                               wintypes.HDC, ctypes.c_void_p, wintypes.COLORREF,
                                               ctypes.c_void_p, wintypes.DWORD]
        self.visible = False

    def present(self, img, alpha=255):
        """img: RGBA PIL (w×h); alpha: 0..255 общая прозрачность."""
        data = img.convert("RGBa").tobytes("raw", "BGRa")
        ctypes.memmove(self.bits, data, len(data))
        pt = wintypes.POINT(self.x, self.y)
        sz = wintypes.SIZE(self.w, self.h)
        src = wintypes.POINT(0, 0)
        bl = _BLEND(0, 0, max(0, min(255, int(alpha))), 1)
        user32.UpdateLayeredWindow(self.hwnd, self.hdc_screen, ctypes.byref(pt), ctypes.byref(sz),
                                   self.hdc, ctypes.byref(src), 0, ctypes.byref(bl), 2)

    def show(self):
        if not self.visible:
            user32.ShowWindow(self.hwnd, 4)  # SW_SHOWNOACTIVATE
            user32.SetWindowPos(self.hwnd, wintypes.HWND(-1), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
            self.visible = True

    def hide(self):
        if self.visible:
            user32.ShowWindow(self.hwnd, 0)
            self.visible = False
