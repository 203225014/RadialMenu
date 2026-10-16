"""Круговое меню (3 сектора) по зажатию боковой кнопки мыши (по умолчанию XButton1 = «назад»).

Зажми кнопку -> в центре экрана появляется круг с тремя секторами.
Реальный курсор остаётся там, где был; меню управляется движением мыши
(курсор скрыт; направление движения от центра выбирает блок).
Блок подсвечивается по направлению движения мыши от центра, курсор скрыт.
У «Действия» снаружи раскрываются «Копировать» и «Вставить».
Отпусти кнопку -> выполняется подсвеченный блок. Отпустил в центре -> отмена.

Запуск:  pythonw radial_menu.py [--button 1|2] [--demo]
  --button 1 = боковая «назад» (по умолчанию), 2 = боковая «вперёд»
  --demo     = показать меню и автоматически «поводить» указателем (без действий)
"""
import ctypes
import math
import os
import queue
import struct
import subprocess
import sys
import threading
import time
import uuid
import tkinter as tk
from ctypes import wintypes

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ring_render as rr  # noqa: E402
import note_widget  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
sys.setswitchinterval(0.001)  # хук должен отвечать Windows быстро (лимит ~300 мс)
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    user32.SetProcessDPIAware()

# ---------------- настройки ----------------
MAIN_D = 117          # расстояние от центра до основных кнопок, px
FAN_D = 116           # расстояние от основной кнопки до кнопок её подменю, px
FAN_SPREAD = 60       # угол между кнопками подменю в веере, °
R_OUT = 156           # дальше этого радиуса от центра выбираются кнопки подменю
RMAX = 168            # указатель не уходит дальше этого радиуса (подменю закрыто)
RMAX_SUB = 246        # то же, когда подменю открыто
SENS = 0.5            # чувствительность мыши внутри виджета
VAULT = "C:/Users/HP/Work"  # хранилище Obsidian: сюда пишутся заметки Ring_NN.md (или --vault путь)
DWELL_T = 0.45        # сек: сколько надо задержаться на «Копировать»/«Вставить», чтобы раскрылись доп. кнопки
HYST = 25             # гистерезис: пока курсор на подпункте, он удерживается ещё на столько px внутрь
DEAD = 34             # мёртвая зона в центре, px (блок активируется за её пределами)
CIRCLE_R = 32         # радиус кнопки (круг 64 px, как в макете)
HUB_R = 17            # радиус маленькой центральной кнопки
HOVER_GROW = 0.08     # на сколько кнопка вырастает при наведении
APPEAR_T = 0.24      # сек: появление круга (вырастает из центра/родителя)
FADE_IN = 0.14        # сек: плавное проявление окна
FADE_OUT = 0.12       # сек: плавное закрытие
COLOR_T = 0.07        # сек: сглаживание цвета (подсветка при переходе)
HIDE_CURSOR = False    # True = прятать системный курсор на время меню (в играх может застрять невидимым)
TICK_MS = 8


CURSOR_IDS = (32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645,
              32646, 32648, 32649, 32650)


def hide_cursor():
    """Подменяет системные курсоры на прозрачный (вернуть: show_cursor)."""
    user32.CreateCursor.restype = ctypes.c_void_p
    user32.CreateCursor.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                    ctypes.c_int, ctypes.c_char_p, ctypes.c_char_p]
    user32.SetSystemCursor.argtypes = [ctypes.c_void_p, wintypes.DWORD]
    for cid in CURSOR_IDS:
        h = user32.CreateCursor(None, 0, 0, 32, 32, b"\xff" * 128, b"\x00" * 128)
        if h:
            user32.SetSystemCursor(h, cid)


def show_cursor():
    user32.SystemParametersInfoW(0x57, 0, None, 0)  # SPI_SETCURSORS: вернуть стандартные


def hex_rgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def rgb_hex(c):
    return "#%02x%02x%02x" % tuple(max(0, min(255, int(round(v)))) for v in c)


def lerp_rgb(a, b, k):
    return tuple(x + (y - x) * k for x, y in zip(a, b))


def ease_back(p, c1=1.1):
    """Появление с лёгким «пружинным» перелётом."""
    p = max(0.0, min(1.0, p))
    c3 = c1 + 1
    return 1 + c3 * (p - 1) ** 3 + c1 * (p - 1) ** 2


def all_nodes():
    out = []

    def walk(n):
        for c in n.children:
            out.append(c)
            walk(c)
    walk(MENU)
    return out


def angdiff(a, b):
    return abs((a - b + 180) % 360 - 180)


class MediaState(threading.Thread):
    """Следит за уровнем звука на устройстве вывода: playing=True, если недавно был звук."""
    THRESHOLD = 0.002   # пик 0..1
    HOLD = 1.5          # сек: пауза между треками не считается выключением

    def __init__(self):
        super().__init__(daemon=True)
        self.playing = False
        self.last_sound = 0.0

    @staticmethod
    def _guid(text):
        return (ctypes.c_ubyte * 16).from_buffer_copy(uuid.UUID(text).bytes_le)

    @staticmethod
    def _fn(obj, index, *argtypes):
        vtbl = ctypes.cast(obj, ctypes.POINTER(ctypes.c_void_p))[0]
        addr = ctypes.cast(vtbl, ctypes.POINTER(ctypes.c_void_p))[index]
        return ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, *argtypes)(addr)

    def _open_meter(self):
        ole32 = ctypes.windll.ole32
        enum = ctypes.c_void_p()
        clsid = self._guid("BCDE0395-E52F-467C-8E3D-C4579291692E")
        iid = self._guid("A95664D2-9614-4F35-A746-DE8DB63617E6")
        if ole32.CoCreateInstance(ctypes.byref(clsid), None, 23, ctypes.byref(iid), ctypes.byref(enum)):
            return None
        dev = ctypes.c_void_p()
        # GetDefaultAudioEndpoint(eRender=0, eConsole=0)
        if self._fn(enum, 4, ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_void_p))(
                enum, 0, 0, ctypes.byref(dev)):
            return None
        meter = ctypes.c_void_p()
        miid = self._guid("C02216F6-8C67-4B5B-9D00-D008E73E0064")
        if self._fn(dev, 3, ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p,
                    ctypes.POINTER(ctypes.c_void_p))(dev, ctypes.byref(miid), 23, None, ctypes.byref(meter)):
            return None
        return meter

    def run(self):
        try:
            ctypes.windll.ole32.CoInitializeEx(None, 0)
        except Exception:
            return
        meter, opened = None, 0.0
        while True:
            try:
                now = time.time()
                if meter is None or now - opened > 10:  # устройство вывода могли сменить
                    meter, opened = self._open_meter(), now
                if meter:
                    peak = ctypes.c_float(0)
                    if self._fn(meter, 3, ctypes.POINTER(ctypes.c_float))(meter, ctypes.byref(peak)):
                        meter = None
                    elif peak.value > self.THRESHOLD:
                        self.last_sound = now
                self.playing = now - self.last_sound < self.HOLD
            except Exception:
                meter = None
            time.sleep(0.1)


media = MediaState()


APP = None  # единственный App (нужен окну заметки как родитель)


def open_note():
    """Окно быстрой заметки для Obsidian (Ctrl+Enter - сохранить в VAULT как Ring_NN.md)."""
    if APP is not None:
        note_widget.NoteWindow(APP.root, VAULT)


def send_win_v():
    """Открыть журнал буфера обмена (Win+V)."""
    LWIN, V, KEYUP = 0x5B, 0x56, 2
    user32.keybd_event(LWIN, 0, 0, 0)
    user32.keybd_event(V, 0, 0, 0)
    user32.keybd_event(V, 0, KEYUP, 0)
    user32.keybd_event(LWIN, 0, KEYUP, 0)


def volume_step(up):
    """Один шаг громкости (как клавиши громкости на клавиатуре; Windows сам покажет индикатор)."""
    vk = 0xAF if up else 0xAE  # VK_VOLUME_UP / VK_VOLUME_DOWN
    user32.keybd_event(vk, 0, 0, 0)
    user32.keybd_event(vk, 0, 2, 0)


def media_key(vk):
    """Нажать медиаклавишу Windows (расширенная клавиша)."""
    user32.keybd_event(vk, 0, 1, 0)
    user32.keybd_event(vk, 0, 1 | 2, 0)


def send_ctrl(vk):
    VK_CONTROL, KEYUP = 0x11, 2
    user32.keybd_event(VK_CONTROL, 0, 0, 0)
    user32.keybd_event(vk, 0, 0, 0)
    user32.keybd_event(vk, 0, KEYUP, 0)
    user32.keybd_event(VK_CONTROL, 0, KEYUP, 0)


class Node:
    def __init__(self, icon, label, action=None, children=None, dyn=None, state=None, dwell=False):
        self.icon, self.label, self.action = icon, label, action
        self.dwell = dwell  # подменю раскрывается только если задержаться на кнопке (сама кнопка тоже выполняется)
        self.dyn = dyn      # () -> (ключ иконки, подпись), если они зависят от ситуации
        self.state = state  # () -> True/False: блок «включён» (зелёный стиль)
        self.p = 0.0        # анимация появления 0..1 (отрицательное = задержка)
        self.hv = 0.0       # анимация наведения 0..1
        self.sw = None      # веса состояний (default/open/hover/on) для плавного перетекания
        self.children = children or []
        self.off = (0.0, 0.0)  # где кнопка стоит относительно центра меню
        self.sector = (0, 0, 0, 0)  # r1, r2, a1, a2 (градусы, y вниз)
        self.parent = None
        for c in self.children:
            c.parent = self


# ---------------- ТУТ МЕНЯЕШЬ МЕНЮ ----------------
MENU = Node("", "", children=[
    Node("square-plus", "Открыть", children=[
        Node("folder", "Проводник", lambda: subprocess.Popen(["explorer.exe"])),
        Node("gear", "Параметры", lambda: os.startfile("ms-settings:")),
        Node("pencil-to-square", "Заметка", lambda: open_note()),
    ]),
    Node("paste", "Вставить", lambda: send_ctrl(0x56), dwell=True, children=[
        Node("clock-arrow-rotate-left", "Буфер обмена", send_win_v),
    ]),
    Node("headphones", "Медиа", state=lambda: media.playing, children=[
        Node("backward-step", "Назад", lambda: media_key(0xB1)),
        Node("pause", "Пауза", lambda: media_key(0xB3),
             dyn=lambda: ("pause", "Пауза") if media.playing else ("play", "Плей")),
        Node("forward-step", "Вперёд", lambda: media_key(0xB0)),
    ]),
    Node("copy", "Копировать", lambda: send_ctrl(0x43), dwell=True, children=[
        Node("square-dashed", "Выбрать все", lambda: send_ctrl(0x41)),
        Node("scissors", "Вырезать", lambda: send_ctrl(0x58)),
    ]),
])
# --------------------------------------------------


def layout_root():
    """Основные кнопки на расстоянии MAIN_D от центра: сверху, справа, снизу, слева (по часовой)."""
    n = len(MENU.children)
    span = 360 / n
    for i, k in enumerate(MENU.children):
        mid = 270 + i * span
        k.sector = (0, 0, mid - span / 2 + 1.5, mid + span / 2 - 1.5)  # угол для выбора по направлению
        k.off = (MAIN_D * math.cos(math.radians(mid)), MAIN_D * math.sin(math.radians(mid)))


def layout_sub(node):
    """Подменю веером наружу от основной кнопки: FAN_D от неё, шаг FAN_SPREAD°."""
    mid = (node.sector[2] + node.sector[3]) / 2
    n = len(node.children)
    ox, oy = node.off

    def spot(off_deg):
        a = math.radians(mid + off_deg)
        return ox + FAN_D * math.cos(a), oy + FAN_D * math.sin(a)

    # порядок кнопок: слева направо (или сверху вниз, если веер горизонтальный)
    vertical = abs(math.sin(math.radians(mid))) > abs(math.cos(math.radians(mid)))
    axis = 0 if vertical else 1
    sign = 1 if spot(FAN_SPREAD)[axis] > spot(-FAN_SPREAD)[axis] else -1
    for j, k in enumerate(node.children):
        k.off = spot((j - (n - 1) / 2) * FAN_SPREAD * sign)
        ang = math.degrees(math.atan2(k.off[1], k.off[0])) % 360  # куда смотреть из центра
        half = min(FAN_SPREAD, 90 / n) / 2
        k.sector = (0, 0, ang - half + 1, ang + half - 1)


def polar(px, py, cx, cy):
    dx, dy = px - cx, py - cy
    return math.hypot(dx, dy), math.degrees(math.atan2(dy, dx)) % 360


def sector_dist(sec, r, phi):
    """Расстояние (px) от точки (r, phi) до сектора; 0 - внутри."""
    r1, r2, a1, a2 = sec
    rad = max(r1 - r, r - r2, 0)
    mid = (a1 + a2) / 2
    half = (a2 - a1) / 2
    d = abs((phi - mid + 180) % 360 - 180)
    arc = max(d - half, 0)
    return math.hypot(rad, math.radians(arc) * max(r, 1))


def clamp_into(sec, r, phi):
    """Ближайшая к (r, phi) точка внутри сектора -> (r, phi)."""
    r1, r2, a1, a2 = sec
    mid, half = (a1 + a2) / 2, (a2 - a1) / 2
    d = (phi - mid + 180) % 360 - 180
    return min(max(r, r1), r2), mid + min(max(d, -half), half)


def centroid(sec, cx, cy):
    r1, r2, a1, a2 = sec
    r, a = (r1 + r2) / 2, math.radians((a1 + a2) / 2)
    return cx + r * math.cos(a), cy + r * math.sin(a)


class Input(threading.Thread):
    """Нижнеуровневый хук: кнопка + сдвиги указателя, пока кнопка зажата."""
    WM_XBUTTONDOWN, WM_XBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL = 0x020B, 0x020C, 0x0200, 0x020A

    def __init__(self, button, q):
        super().__init__(daemon=True)
        self.button, self.q = button, q
        self.pass_inject = 0.0  # до этого времени синтетические события кнопки не глотаем
        self.capture = False
        self.lock = threading.Lock()
        self.dx = self.dy = 0
        self.last = (0, 0)     # предыдущая позиция указателя
        self.frozen = (0, 0)   # где нажали кнопку
        # локально: курсор вообще не двигается (движение глотаем), дельты берём из Raw Input;
        # по RDP raw-дельт нет, там считаем по координатам указателя
        self.raw_ok = True
        self.use_raw = not user32.GetSystemMetrics(0x1000)

    def take(self):
        with self.lock:
            d = (self.dx, self.dy)
            self.dx = self.dy = 0
        return d

    def rebase(self, pos):
        with self.lock:
            self.last = pos

    def run(self):
        class MSLL(ctypes.Structure):
            _fields_ = [("pt", wintypes.POINT), ("mouseData", wintypes.DWORD),
                        ("flags", wintypes.DWORD), ("time", wintypes.DWORD),
                        ("extra", ctypes.c_void_p)]

        HOOK = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM)
        user32.CallNextHookEx.argtypes = [wintypes.HHOOK, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM]
        user32.CallNextHookEx.restype = ctypes.c_ssize_t
        user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOK, wintypes.HINSTANCE, wintypes.DWORD]
        user32.SetWindowsHookExW.restype = wintypes.HHOOK
        kernel32.GetModuleHandleW.restype = wintypes.HMODULE
        kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]

        def hook_proc(nCode, wParam, lParam):
            if nCode == 0:
                if wParam in (self.WM_XBUTTONDOWN, self.WM_XBUTTONUP):
                    info = ctypes.cast(lParam, ctypes.POINTER(MSLL)).contents
                    if (info.mouseData >> 16) & 0xFFFF == self.button:
                        if info.flags & 1 and time.perf_counter() < self.pass_inject:
                            return user32.CallNextHookEx(None, nCode, wParam, lParam)
                        if wParam == self.WM_XBUTTONDOWN:
                            with self.lock:
                                self.dx = self.dy = 0
                                self.frozen = self.last = (info.pt.x, info.pt.y)
                            # режим выбираем на каждое нажатие: RDP мог подключиться позже запуска
                            self.use_raw = self.raw_ok and not user32.GetSystemMetrics(0x1000)
                            self.capture = True
                            self.q.put("down")
                        else:
                            self.capture = False
                            self.q.put("up")
                        return 1
                elif wParam == self.WM_MOUSEWHEEL and self.capture:
                    info = ctypes.cast(lParam, ctypes.POINTER(MSLL)).contents
                    delta = ctypes.c_short((info.mouseData >> 16) & 0xFFFF).value
                    if delta:
                        self.q.put("vol_up" if delta > 0 else "vol_down")
                    return 1  # колесо в это время не крутит страницы
                elif wParam == self.WM_MOUSEMOVE and self.capture:
                    if self.use_raw:
                        return 1  # курсор остаётся на месте
                    info = ctypes.cast(lParam, ctypes.POINTER(MSLL)).contents
                    own = info.flags & 1 and (info.pt.x, info.pt.y) == self.frozen  # наш SetCursorPos
                    if not own:
                        with self.lock:
                            self.dx += info.pt.x - self.last[0]
                            self.dy += info.pt.y - self.last[1]
                            self.last = (info.pt.x, info.pt.y)
            return user32.CallNextHookEx(None, nCode, wParam, lParam)

        self._hook_proc = HOOK(hook_proc)
        hinst = kernel32.GetModuleHandleW(None)

        def install():
            self._hook = user32.SetWindowsHookExW(14, self._hook_proc, hinst, 0)
            if not self._hook:
                raise OSError(f"SetWindowsHookEx failed: {ctypes.GetLastError()}")

        install()
        user32.UnhookWindowsHookEx.argtypes = [wintypes.HHOOK]
        kernel32.GetCurrentThread.restype = ctypes.c_void_p
        kernel32.SetThreadPriority.argtypes = [ctypes.c_void_p, ctypes.c_int]
        kernel32.SetThreadPriority(kernel32.GetCurrentThread(), 2)  # THREAD_PRIORITY_HIGHEST
        user32.SetTimer(None, 0, 2000, None)
        if self.use_raw:
            try:
                self._setup_raw(wintypes)
            except Exception:
                self.raw_ok = self.use_raw = False  # не вышло - работаем по координатам

        msg = wintypes.MSG()
        while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            if msg.message == 0x0113:  # WM_TIMER: Windows мог снять хук после долгого ответа - ставим заново
                if not self.capture:
                    try:
                        user32.UnhookWindowsHookEx(self._hook)
                        install()
                    except OSError:
                        pass
                continue
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

    def _setup_raw(self, wintypes):
        class WNDCLASS(ctypes.Structure):
            _fields_ = [("style", wintypes.UINT), ("lpfnWndProc", ctypes.c_void_p),
                        ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                        ("hInstance", wintypes.HINSTANCE), ("hIcon", wintypes.HANDLE),
                        ("hCursor", wintypes.HANDLE), ("hbrBackground", wintypes.HANDLE),
                        ("lpszMenuName", wintypes.LPCWSTR), ("lpszClassName", wintypes.LPCWSTR)]

        class RID(ctypes.Structure):
            _fields_ = [("usUsagePage", wintypes.USHORT), ("usUsage", wintypes.USHORT),
                        ("dwFlags", wintypes.DWORD), ("hwndTarget", wintypes.HWND)]

        WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT,
                                     wintypes.WPARAM, wintypes.LPARAM)
        user32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
        user32.DefWindowProcW.restype = ctypes.c_ssize_t
        user32.GetRawInputData.argtypes = [ctypes.c_void_p, wintypes.UINT, ctypes.c_void_p,
                                           ctypes.POINTER(wintypes.UINT), wintypes.UINT]
        user32.GetRawInputData.restype = wintypes.UINT
        user32.CreateWindowExW.restype = wintypes.HWND
        user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR,
                                           wintypes.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                           ctypes.c_int, wintypes.HWND, wintypes.HANDLE,
                                           wintypes.HINSTANCE, ctypes.c_void_p]
        hinst = kernel32.GetModuleHandleW(None)

        def wnd_proc(hwnd, msg, wParam, lParam):
            if msg == 0x00FF:  # WM_INPUT
                if self.capture:
                    size = wintypes.UINT(0)
                    user32.GetRawInputData(lParam, 0x10000003, None, ctypes.byref(size), 24)
                    buf = ctypes.create_string_buffer(size.value)
                    if user32.GetRawInputData(lParam, 0x10000003, buf, ctypes.byref(size), 24) == size.value:
                        if struct.unpack_from("<I", buf, 0)[0] == 0:  # RIM_TYPEMOUSE
                            flags = struct.unpack_from("<H", buf, 24)[0]
                            if not flags & 1:  # относительное движение
                                x, y = struct.unpack_from("<ii", buf, 36)
                                with self.lock:
                                    self.dx += x
                                    self.dy += y
                return 0
            return user32.DefWindowProcW(hwnd, msg, wParam, lParam)

        self._wnd_proc = WNDPROC(wnd_proc)
        wc = WNDCLASS()
        wc.lpfnWndProc = ctypes.cast(self._wnd_proc, ctypes.c_void_p)
        wc.hInstance = hinst
        wc.lpszClassName = "RadialMenuRawInput"
        user32.RegisterClassW(ctypes.byref(wc))
        hwnd = user32.CreateWindowExW(0, "RadialMenuRawInput", "", 0, 0, 0, 0, 0,
                                      wintypes.HWND(-3), None, hinst, None)  # HWND_MESSAGE
        rid = RID(1, 2, 0x100, hwnd)  # мышь, RIDEV_INPUTSINK
        if not hwnd or not user32.RegisterRawInputDevices(ctypes.byref(rid), 1, ctypes.sizeof(rid)):
            raise OSError("raw input")


class App:
    ICON_KEYS = ("folder", "gear", "square-plus", "copy", "paste", "file", "headphones",
                 "backward-step", "forward-step", "pause", "play", "cross", "blank",
                 "file-text", "scissors", "square-dashed", "clock-arrow-rotate-left", "pencil-to-square")

    def __init__(self, button, demo):
        global APP
        APP = self
        self.q = queue.Queue()
        self.inp = Input(button, self.q)
        self.inp.start()
        self.demo = demo
        self.stuck = 0
        self.vk = 0x05 if button == 1 else 0x06  # VK_XBUTTON1 / VK_XBUTTON2
        self.leak_t = 0.0
        self.root = tk.Tk()
        self.root.withdraw()  # окно Tk нужно только как цикл событий; рисует Layer
        self.sw, self.sh = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)  # главный монитор
        self.cx, self.cy = self.sw / 2, self.sh / 2  # центр меню: центр главного монитора
        global MAIN_D, FAN_D, R_OUT, RMAX, RMAX_SUB, DEAD, CIRCLE_R, HUB_R
        k = min(1.0, (min(self.sw, self.sh) / 2 - 24) / (MAIN_D + FAN_D + CIRCLE_R * 1.2 + 24))
        MAIN_D, FAN_D, R_OUT, RMAX, RMAX_SUB, DEAD, CIRCLE_R, HUB_R = (
            v * k for v in (MAIN_D, FAN_D, R_OUT, RMAX, RMAX_SUB, DEAD, CIRCLE_R, HUB_R))  # вписать в экран
        self.half = int(MAIN_D + FAN_D + CIRCLE_R * (1 + HOVER_GROW) * 1.2 + 24)
        self.layer = rr.Layer(self.cx - self.half, self.cy - self.half, 2 * self.half, 2 * self.half)
        for st in rr.STATES:  # все кнопки строим заранее, чтобы первый показ не подвисал
            for key in self.ICON_KEYS:
                rr.sprite(st, key)
        self.active = False
        self.fading = False
        self.path = []
        self.hot = None
        self.px = self.py = 0.0
        self.t = 0
        self.hub_p = 0.0
        self.hub_sw = None
        self.toast = None
        self.dw_root = None
        self.dw_t0 = 0.0
        self.t_prev = time.perf_counter()
        self.show_t = self.fade_t = 0.0
        layout_root()
        media.start()
        self.root.after(TICK_MS, self.tick)
        if demo:
            self.root.after(800, lambda: self.q.put("down"))

    def show(self):
        self.path, self.hot, self.t = [], None, 0
        self.stuck = 0
        self.px, self.py = self.cx, self.cy  # виртуальный указатель в центре
        self.toast = None
        self.dw_root = None
        self.inp.take()
        self.active = True
        self.fading = False
        if HIDE_CURSOR:
            hide_cursor()
        for i, n in enumerate(MENU.children):
            n.p = -0.10 * i  # круги появляются по очереди
        for n in all_nodes():
            if n.parent is not MENU:
                n.p = 0.0
            n.hv, n.sw = 0.0, None
        self.hub_p, self.hub_sw = 0.0, None
        self.show_t = self.t_prev = time.perf_counter()
        self.layer.present(Image.new("RGBA", (2 * self.half, 2 * self.half), (0, 0, 0, 0)), 0)
        self.layer.show()

    def hide(self):
        self.active = False
        self.fading = True  # гаснет плавно, потом layer.hide() в tick
        self.fade_t = time.perf_counter()
        if HIDE_CURSOR:
            show_cursor()

    def visible_nodes(self):
        out = list(MENU.children)
        for n in self.path:
            out += n.children
        return out

    def tick(self):
        try:
            while True:
                ev = self.q.get_nowait()
                if ev == "down" and not self.active:
                    self.show()
                elif ev in ("vol_up", "vol_down") and self.active:
                    volume_step(ev == "vol_up")
                    self.toast = ("Громкость +" if ev == "vol_up" else "Громкость −", time.perf_counter() + 0.9)
                elif ev == "up" and self.active:
                    node = self.hot
                    self.hide()
                    if node and node.action:
                        self.root.after(30, node.action)
        except queue.Empty:
            pass
        if not self.active and not self.inp.capture:
            # сторож: хук глотает нашу кнопку целиком, так что «зажата» без меню - это утечка (хук не успел ответить);
            # отпускаем её сами, иначе клики перестают работать
            if user32.GetAsyncKeyState(self.vk) & 0x8000:
                now = time.perf_counter()
                if not self.leak_t:
                    self.leak_t = now
                elif now - self.leak_t > 0.5:
                    self.inp.pass_inject = now + 0.5
                    user32.mouse_event(0x100, 0, 0, self.inp.button, 0)  # XBUTTONUP
                    self.leak_t = 0.0
            else:
                self.leak_t = 0.0
        if self.active and not self.demo:
            # предохранитель: событие «up» могло потеряться - через 20 с меню закрывается само
            self.stuck += 1
            if self.stuck * TICK_MS > 20000:
                self.inp.capture = False
                self.q.put("up")
        if self.active:
            self.update_menu()
            if not user32.GetSystemMetrics(0x1000) and not self.inp.use_raw:
                user32.SetCursorPos(*self.inp.frozen)
                self.inp.rebase(self.inp.frozen)
        if self.active or self.fading:
            now = time.perf_counter()
            dt = min(now - self.t_prev, 0.05)
            self.t_prev = now
            self.animate(dt)
            if self.active:
                g = min(1.0, (now - self.show_t) / FADE_IN)
            else:
                g = max(0.0, 1.0 - (now - self.fade_t) / FADE_OUT)
            self.draw(255 * g)
            if not self.active and g <= 0.0:
                self.fading = False
                self.layer.hide()
        self.root.after(TICK_MS, self.tick)

    def demo_target(self):
        """Автодвижение указателя для --demo: Проводник -> Параметры -> Действия -> Копировать -> Вставить."""
        names = ["Проводник", "Параметры", "Действия", "Копировать", "Вставить"]
        idx = (self.t // 100) % len(names)
        for n in self.visible_nodes():
            if n.label == names[idx]:
                return self.cx + n.off[0], self.cy + n.off[1]
        return self.cx, self.cy

    def update_menu(self):
        if self.demo:
            tx, ty = self.demo_target()
            self.px += (tx - self.px) * 0.08
            self.py += (ty - self.py) * 0.08
            self.t += 1
        else:
            dx, dy = self.inp.take()
            self.px += dx * SENS
            self.py += dy * SENS
        r, phi = polar(self.px, self.py, self.cx, self.cy)
        prev = self.path[0] if self.path else None
        rmax = RMAX_SUB if prev else RMAX
        if r > rmax:  # указатель (невидимый) не уходит дальше края меню
            self.px = self.cx + (self.px - self.cx) * rmax / r
            self.py = self.cy + (self.py - self.cy) * rmax / r
            r = rmax
        hot = root = None
        if r >= DEAD:  # блок выбирается по направлению от центра
            root = min(MENU.children,
                       key=lambda n: angdiff(phi, (n.sector[2] + n.sector[3]) / 2))
            hot = root
            if root.children:
                layout_sub(root)
            if root.children and (not root.dwell or root in self.path):  # подпункты выбираются, только когда раскрыты
                # если уже стоим на подпункте этого блока - порог выхода из подменю ниже
                keep = self.hot is not None and self.hot.parent is root
                if r > R_OUT - (HYST if keep else 0):
                    # ближайший по углу подпункт; зазоры между секторами не считаются «пустотой»
                    k = min(root.children, key=lambda c: angdiff(phi, (c.sector[2] + c.sector[3]) / 2))
                    if angdiff(phi, (k.sector[2] + k.sector[3]) / 2) <= (k.sector[3] - k.sector[2]) / 2 + 1.5:
                        hot = k
        if r < DEAD and not self.path:
            self.dw_root = None  # вернулись в центр, ничего не открыто: отсчёт задержки заново
        if r >= DEAD:  # в центре подменю не трогаем: оно закрывается только переходом на другой блок
            now = time.perf_counter()
            if root is not self.dw_root:
                self.dw_root, self.dw_t0 = root, now
            opened = root in self.path
            ready = (not root.dwell) or opened or (now - self.dw_t0 >= DWELL_T)
            self.path = [root] if (root.children and ready) else []
        self.hot = hot

    STATE_NAMES = ("default", "open", "hover", "on")

    @staticmethod
    def _step(weights, target, k):
        if weights is None:
            return {s: 1.0 if s == target else 0.0 for s in App.STATE_NAMES}
        for s in App.STATE_NAMES:
            weights[s] += ((1.0 if s == target else 0.0) - weights[s]) * k
        return weights

    def animate(self, dt):
        """Плавно подтягивает p / hv / веса состояний каждой кнопки к целевым значениям."""
        vis = set(self.visible_nodes()) if self.active else set()
        k = 1 - math.exp(-dt / COLOR_T)
        for n in all_nodes():
            if n in vis:
                n.p = min(1.0, n.p + dt / APPEAR_T)
            else:
                n.p = max(0.0, n.p - dt / (APPEAR_T * 0.6)) if n.p > 0 else 0.0
            hot = self.active and n is self.hot and (not n.children or bool(n.action))
            if hot:
                target = "hover"
            elif n in self.path:
                target = "open"
            elif n.state and n.state():
                target = "on"
            else:
                target = "default"
            n.sw = self._step(n.sw, target, k)
            n.hv += ((1.0 if hot else 0.0) - n.hv) * k
        self.hub_p = min(1.0, self.hub_p + dt / APPEAR_T) if self.active else max(0.0, self.hub_p - dt / (APPEAR_T * 0.6))
        self.hub_sw = self._step(self.hub_sw, "open" if (self.hot is None and self.active) else "default", k)

    @staticmethod
    def _blend(weights, icon):
        """Спрайт кнопки: смесь состояний по весам (плавный переход цвета)."""
        items = sorted(((w, s) for s, w in weights.items() if w > 0.01), reverse=True)
        if not items:
            return rr.sprite("default", icon)
        acc = rr.sprite(items[0][1], icon)
        total = items[0][0]
        for w, s in items[1:]:
            acc = Image.blend(acc, rr.sprite(s, icon), w / (total + w))
            total += w
        return acc

    @staticmethod
    def _put(img, sp, size, x, y, alpha=1.0):
        """Положить спрайт, масштабируя до size px, центр в (x, y)."""
        if size < 6:
            return
        if size != sp.width:
            sp = rr._premul_resize(sp, (size, size), Image.BICUBIC if size > sp.width else Image.LANCZOS)
        if alpha < 0.999:
            sp = sp.copy()
            sp.putalpha(sp.getchannel("A").point(lambda v: int(v * alpha)))
        dx, dy = int(round(x - size / 2)), int(round(y - size / 2))
        if dx < 0 or dy < 0 or dx + size > img.width or dy + size > img.height:
            return
        img.alpha_composite(sp, (dx, dy))

    def draw(self, alpha255):
        H = self.half
        img = Image.new("RGBA", (2 * H, 2 * H), (0, 0, 0, 0))
        pos = {}

        def pos_of(n):
            if n in pos:
                return pos[n]
            fx, fy = H + n.off[0], H + n.off[1]
            sx, sy = (H, H) if n.parent is MENU else pos_of(n.parent)
            e = ease_back(n.p)
            pos[n] = (sx + (fx - sx) * e, sy + (fy - sy) * e)
            return pos[n]

        nodes = [n for n in all_nodes() if n.p > 0.01]
        # центр: маленькая серая кнопка и синяя точка - куда сейчас смотрит мышь
        hp = ease_back(self.hub_p)
        hs = int(round(rr.BOX * (HUB_R * hp) / (rr.DIAM / 2)))
        if hs >= 6:
            a = min(1.0, self.hub_p * 2.5)
            self._put(img, rr.sprite("default", "blank"), hs, H, H, a)
            dx, dy = self.px - self.cx, self.py - self.cy
            L = math.hypot(dx, dy)
            reach = HUB_R * 0.45
            if L > reach:
                dx, dy = dx * reach / L, dy * reach / L
            ds = int(round(rr.DOT_BOX * hp))
            if ds >= 4:
                self._put(img, rr.dot_sprite(), ds, H + dx, H + dy, a)
        # кнопки
        for n in nodes:
            e = ease_back(n.p)
            rad = max(0.0, CIRCLE_R * (1 + HOVER_GROW * n.hv) * e)
            icon, label = n.dyn() if n.dyn else (n.icon, n.label)
            x, y = pos_of(n)
            self._put(img, self._blend(n.sw, icon), int(round(rr.BOX * rad / (rr.DIAM / 2))), x, y,
                      min(1.0, max(0.0, n.p * 2.5)))
        # подпись выбранной кнопки - со стороны центра, в промежутке между кнопками
        for n in nodes:
            if n.hv > 0.15:
                icon, label = n.dyn() if n.dyn else (n.icon, n.label)
                if n.state:
                    label += " · играет" if n.state() else " · выкл"
                x, y = pos_of(n)
                pill = rr.label_pill(label, alpha=min(1.0, n.hv * 1.5))
                rad = CIRCLE_R * (1 + HOVER_GROW * n.hv)
                py = y + rad + 6 if y <= H else y - rad - 6 - pill.height
                px = max(2, min(2 * H - pill.width - 2, x - pill.width / 2))
                py = max(2, min(2 * H - pill.height - 2, py))
                img.alpha_composite(pill, (int(px), int(py)))
        # подпись при смене громкости - под центром, пока не погаснет
        if self.toast:
            text, until = self.toast
            left = until - time.perf_counter()
            if left <= 0:
                self.toast = None
            else:
                pill = rr.label_pill(text, alpha=min(1.0, left / 0.3), size=14)
                img.alpha_composite(pill, (int(H - pill.width / 2), int(H + HUB_R + 14)))
        self.layer.present(img, alpha255)

    def run(self):
        self.root.mainloop()


import atexit
atexit.register(show_cursor)

def _log_error(exc_text):
    try:
        path = os.path.join(os.environ.get("TEMP", HERE), "RadialMenu_error.log")
        with open(path, "a", encoding="utf-8") as f:
            f.write(time.strftime("%Y-%m-%d %H:%M:%S") + "\n" + exc_text + "\n")
    except OSError:
        pass


if __name__ == "__main__":
    # один экземпляр: два хука на одну кнопку мешают друг другу
    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _k32.CreateMutexW.restype = ctypes.c_void_p
    _k32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_bool, ctypes.c_wchar_p]
    _mutex = _k32.CreateMutexW(None, False, "Local\\RadialMenuSingleton_HP")
    if ctypes.get_last_error() == 183:  # ERROR_ALREADY_EXISTS
        sys.exit(0)
    show_cursor()
    btn = 1
    if "--button" in sys.argv:
        btn = int(sys.argv[sys.argv.index("--button") + 1])
    if "--vault" in sys.argv:
        VAULT = sys.argv[sys.argv.index("--vault") + 1]
    import traceback

    def _thread_hook(args):  # ошибки фоновых потоков (хук, звук) - в лог, иначе в exe без консоли их не видно
        _log_error("поток: " + "".join(traceback.format_exception(args.exc_type, args.exc_value, args.exc_traceback)))

    threading.excepthook = _thread_hook
    try:
        app = App(btn, "--demo" in sys.argv)
        app.root.report_callback_exception = lambda *a: _log_error("tk: " + "".join(traceback.format_exception(*a)))
        app.run()
    except Exception:
        _log_error(traceback.format_exc())
        raise
