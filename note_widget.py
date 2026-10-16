"""Мини-окно быстрой заметки для Obsidian.

Пишешь текст -> Ctrl+Enter -> файл Ring_NN.md появляется в корне хранилища (Obsidian подхватывает сам).
Esc - отмена. Как в Obsidian: набрал [[ - появляется список заметок, фильтруется по набору,
стрелки/Enter/Tab подставляют ссылку, [[ автоматически закрывается ]].
"""
import ctypes
import os
import re
import tkinter as tk
from tkinter import font as tkfont

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

BG = "#1b1f2b"
BG2 = "#262c3d"
EDGE = "#8fa6d6"
FG = "#ffffff"
DIM = "#8791a8"
ACC = "#699bff"
LINK_RE = re.compile(r"\[\[([^\[\]|#\n]*)$")  # незакрытая ссылка перед курсором

_current = None  # открытое окно заметки (одно на всех)


def scan_names(vault):
    """[(показываемое имя, текст ссылки, mtime)] - заметки .md и .canvas, свежие выше."""
    found = []
    for dp, dn, fn in os.walk(vault):
        dn[:] = [d for d in dn if not d.startswith(".")]
        for f in fn:
            low = f.lower()
            if low.endswith(".md"):
                base = f[:-3]
            elif low.endswith(".canvas"):
                base = f  # у canvas в ссылке остаётся расширение
            else:
                continue
            full = os.path.join(dp, f)
            try:
                mt = os.path.getmtime(full)
            except OSError:
                mt = 0
            found.append((base, os.path.relpath(full, vault).replace("\\", "/"), mt))
    count = {}
    for b, _, _ in found:
        count[b] = count.get(b, 0) + 1
    res = []
    for b, rel, mt in found:
        link = b if count[b] == 1 else (rel[:-3] if rel.lower().endswith(".md") else rel)
        res.append((b, link, mt))
    res.sort(key=lambda t: -t[2])
    return res


def next_ring_number(vault):
    mx = 0
    for dp, dn, fn in os.walk(vault):
        dn[:] = [d for d in dn if not d.startswith(".")]
        for f in fn:
            m = re.fullmatch(r"Ring_(\d+)\.md", f, re.I)
            if m:
                mx = max(mx, int(m.group(1)))
    return mx + 1


def ring_name(n):
    return f"Ring_{n:02d}"


class NoteWindow:
    W, H = 500, 270

    def __init__(self, root, vault):
        global _current
        if _current is not None:
            try:
                if _current.win.winfo_exists():
                    _current._focus()
                    return
            except tk.TclError:
                pass
        _current = self
        self.vault = vault
        self.ok = os.path.isdir(vault)
        self.names = scan_names(vault) if self.ok else []
        self.cur_items = []
        self.number = next_ring_number(vault) if self.ok else 0

        w = self.win = tk.Toplevel(root)
        w.overrideredirect(True)
        w.attributes("-topmost", True, "-alpha", 0.94)
        sw, sh = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
        w.geometry(f"{self.W}x{self.H}+{(sw - self.W) // 2}+{(sh - self.H) // 2 - 40}")
        w.configure(bg=EDGE)
        body = tk.Frame(w, bg=BG)
        body.pack(fill="both", expand=True, padx=1, pady=1)

        self.f_title = tkfont.Font(family="Segoe UI", size=11, weight="bold")
        self.f_text = tkfont.Font(family="Segoe UI", size=12)
        self.f_small = tkfont.Font(family="Segoe UI", size=9)

        head = tk.Frame(body, bg=BG)
        head.pack(fill="x", padx=14, pady=(10, 4))
        self.title = tk.Label(head, text=ring_name(self.number) if self.ok else "Нет хранилища",
                              bg=BG, fg=ACC, font=self.f_title)
        self.title.pack(side="left")
        tk.Label(head, text="новая заметка", bg=BG, fg=DIM, font=self.f_small).pack(side="left", padx=8)
        for wd in (head, self.title):
            wd.bind("<ButtonPress-1>", self._drag_start)
            wd.bind("<B1-Motion>", self._drag_move)

        foot = tk.Frame(body, bg=BG)
        foot.pack(side="bottom", fill="x", padx=14, pady=(2, 8))
        self.foot = tk.Label(foot, text="Ctrl+Enter - сохранить    Esc - отмена    [[ - ссылка на заметку",
                             bg=BG, fg=DIM, font=self.f_small)
        self.foot.pack(side="left")
        self.text = tk.Text(body, bg=BG, fg=FG, insertbackground=FG, relief="flat", bd=0,
                            font=self.f_text, wrap="word", undo=True, padx=14, pady=4, height=4,
                            selectbackground=ACC, selectforeground=FG, highlightthickness=0)
        self.text.pack(fill="both", expand=True)
        if not self.ok:
            self.text.insert("1.0", f"Папка хранилища не найдена:\n{vault}")
            self.text.configure(state="disabled")

        # список подсказок для [[ - поверх текста, без отдельного окна
        self.lb = tk.Listbox(body, bg=BG2, fg=FG, selectbackground=ACC, selectforeground=FG,
                             activestyle="none", relief="flat", bd=0, font=self.f_text,
                             highlightthickness=1, highlightbackground=EDGE, highlightcolor=EDGE,
                             exportselection=False, height=6, width=34)
        self.lb.bind("<ButtonRelease-1>", lambda e: self._accept())

        t = self.text
        t.bind("<KeyPress>", self._on_key)
        t.bind("<KeyRelease>", self._on_key_release)
        t.bind("<ButtonRelease-1>", lambda e: self._suggest_update())
        t.bind("<Control-Return>", lambda e: (self.save(), "break")[1])
        t.bind("<Escape>", self._on_escape)
        t.bind("<Control-KeyPress>", self._on_ctrl)  # Ctrl+C/V/X/A/Z на любой раскладке
        w.after(30, self._focus)

    # ------------------------------------------------------------ окно
    def _drag_start(self, e):
        self._dx, self._dy = e.x_root - self.win.winfo_x(), e.y_root - self.win.winfo_y()

    def _drag_move(self, e):
        self.win.geometry(f"+{e.x_root - self._dx}+{e.y_root - self._dy}")

    def _focus(self):
        """Отнять клавиатурный фокус у текущего окна (Windows не любит, когда фокус «крадут»)."""
        w = self.win
        w.update_idletasks()
        hwnd = user32.GetParent(w.winfo_id()) or w.winfo_id()
        fg = user32.GetForegroundWindow()
        cur = kernel32.GetCurrentThreadId()
        fg_thr = user32.GetWindowThreadProcessId(fg, None)
        try:
            user32.AttachThreadInput(fg_thr, cur, True)
            user32.SetForegroundWindow(hwnd)
            user32.BringWindowToTop(hwnd)
            user32.SetFocus(hwnd)
        finally:
            user32.AttachThreadInput(fg_thr, cur, False)
        w.lift()
        self.text.focus_force()

    def close(self):
        global _current
        _current = None
        try:
            self.win.destroy()
        except tk.TclError:
            pass

    # ------------------------------------------------------------ сохранение
    def save(self):
        if not self.ok:
            return self.close()
        content = self.text.get("1.0", "end-1c").rstrip()
        if not content:
            return self.close()
        name = None
        for _ in range(20):  # на случай гонки с другим файлом
            name = ring_name(next_ring_number(self.vault))
            try:
                with open(os.path.join(self.vault, name + ".md"), "x", encoding="utf-8", newline="\n") as f:
                    f.write(content + "\n")
                break
            except FileExistsError:
                name = None
        self.hide_list()
        self.text.configure(state="disabled")
        self.title.configure(text=f"Сохранено: {name}" if name else "Не удалось сохранить",
                             fg="#7fe0a3" if name else "#ff8a8a")
        self.win.after(650, self.close)

    # ------------------------------------------------------------ клавиши
    def _on_ctrl(self, e):
        k = e.keycode
        if k == 67:
            self.text.event_generate("<<Copy>>")
        elif k == 86:
            self.text.event_generate("<<Paste>>")
        elif k == 88:
            self.text.event_generate("<<Cut>>")
        elif k == 65:
            self.text.tag_add("sel", "1.0", "end-1c")
            self.text.mark_set("insert", "end-1c")
        elif k == 90:
            try:
                self.text.edit_undo()
            except tk.TclError:
                pass
        else:
            return None
        return "break"

    def _on_escape(self, e):
        if self.lb.winfo_ismapped():
            self.hide_list()
        else:
            self.close()
        return "break"

    def _on_key(self, e):
        if self.lb.winfo_ismapped():
            if e.keysym == "Up":
                self._move(-1)
                return "break"
            if e.keysym == "Down":
                self._move(1)
                return "break"
            if e.keysym in ("Return", "Tab"):
                self._accept()
                return "break"
        if e.char == "[" and self.text.get("insert-1c", "insert") == "[":
            self.text.insert("insert", "[]]")  # [[ -> [[]] с курсором внутри
            self.text.mark_set("insert", "insert-2c")
            self.win.after(1, self._suggest_update)
            return "break"
        return None

    def _on_key_release(self, e):
        if e.keysym in ("Up", "Down", "Return", "Tab", "Escape", "Shift_L", "Shift_R", "Control_L", "Control_R"):
            return
        self._suggest_update()

    # ------------------------------------------------------------ подсказки [[
    def _suggest_update(self):
        if not self.ok:
            return
        before = self.text.get(self.text.index("insert linestart"), "insert")
        m = LINK_RE.search(before)
        if not m:
            return self.hide_list()
        q = m.group(1).lower()
        items = [n for n in self.names if q in n[0].lower()]
        items.sort(key=lambda n: not n[0].lower().startswith(q))  # сначала начинающиеся с набранного
        self.cur_items = items[:80]
        if not self.cur_items:
            return self.hide_list()
        self.lb.delete(0, "end")
        for n in self.cur_items:
            self.lb.insert("end", n[0])
        self.lb.selection_clear(0, "end")
        self.lb.selection_set(0)
        self.lb.configure(height=min(6, len(self.cur_items)))
        self._place_list()

    def _place_list(self):
        self.win.update_idletasks()
        bb = self.text.bbox("insert")
        if not bb:
            return
        x = self.text.winfo_x() + bb[0]
        y = self.text.winfo_y() + bb[1] + bb[3] + 2
        lw, lh = self.lb.winfo_reqwidth(), self.lb.winfo_reqheight()
        bw = self.win.winfo_width() - 2
        bh = self.win.winfo_height() - 2
        x = max(4, min(x, bw - lw - 4))
        if y + lh > bh - 4:  # не помещается снизу - над строкой
            y = max(4, self.text.winfo_y() + bb[1] - lh - 2)
        self.lb.place(x=x, y=y)
        self.lb.lift()

    def hide_list(self):
        self.lb.place_forget()

    def _move(self, d):
        n = self.lb.size()
        if not n:
            return
        sel = self.lb.curselection()
        i = (sel[0] if sel else 0) + d
        i = max(0, min(n - 1, i))
        self.lb.selection_clear(0, "end")
        self.lb.selection_set(i)
        self.lb.see(i)

    def _accept(self):
        sel = self.lb.curselection()
        if not sel or not self.cur_items:
            return self.hide_list()
        link = self.cur_items[sel[0]][1]
        before = self.text.get(self.text.index("insert linestart"), "insert")
        m = LINK_RE.search(before)
        if not m:
            return self.hide_list()
        line = self.text.index("insert").split(".")[0]
        self.text.delete(f"{line}.{m.start(1)}", "insert")
        self.text.insert("insert", link)
        if self.text.get("insert", "insert+2c") == "]]":
            self.text.mark_set("insert", "insert+2c")
        else:
            self.text.insert("insert", "]]")
        self.hide_list()
        self.text.focus_set()
