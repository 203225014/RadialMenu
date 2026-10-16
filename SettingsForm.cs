using System.Drawing;
using System.Windows.Forms;

namespace RadialMenu;

/// <summary>Окно настроек: кнопка мыши, чувствительность, размер, задержка, хранилище Obsidian, автозапуск, пункты меню.</summary>
internal sealed class SettingsForm : Form
{
    static SettingsForm _current;

    public static void OpenOrFocus(Action<Settings, bool> onSave)
    {
        if (_current != null && !_current.IsDisposed) { _current.Activate(); return; }
        _current = new SettingsForm(onSave);
        _current.Show();
        _current.Activate();
    }

    readonly Action<Settings, bool> _onSave;
    readonly ComboBox _button = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 260 };
    readonly TrackBar _sens = Track(20, 300, 10);
    readonly TrackBar _size = Track(50, 150, 5);
    readonly TrackBar _dwell = Track(200, 1500, 50);
    readonly Label _sensL = new() { AutoSize = true }, _sizeL = new() { AutoSize = true }, _dwellL = new() { AutoSize = true };
    readonly TextBox _vault = new() { Width = 300 };
    readonly CheckBox _auto = new() { Text = "Запускать вместе с Windows", AutoSize = true };
    readonly TreeView _tree = new() { CheckBoxes = true, Height = 190, Width = 420, ShowLines = false, ShowRootLines = false };

    static TrackBar Track(int min, int max, int step) =>
        new() { Minimum = min, Maximum = max, SmallChange = step, LargeChange = step * 2, TickFrequency = step * 4, Width = 300, AutoSize = false, Height = 32 };

    SettingsForm(Action<Settings, bool> onSave)
    {
        _onSave = onSave;
        Text = "Радиальное меню - настройки";
        Font = new Font("Segoe UI", 9.5f);
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        ShowInTaskbar = true;
        AutoScaleMode = AutoScaleMode.Dpi;
        AutoScaleDimensions = new SizeF(96, 96);
        try { Icon = new Icon(typeof(SettingsForm).Assembly.GetManifestResourceStream(
            typeof(SettingsForm).Assembly.GetManifestResourceNames().First(n => n.EndsWith(".RadialMenu.ico")))); } catch { }

        var s = Cfg.S;
        _button.Items.AddRange(new object[] { "Назад (боковая кнопка 1)", "Вперёд (боковая кнопка 2)" });
        _button.SelectedIndex = s.Button == 2 ? 1 : 0;
        _sens.Value = s.SensPercent; _size.Value = s.SizePercent; _dwell.Value = Math.Clamp(s.DwellMs, 200, 1500);
        _vault.Text = s.Vault;
        _auto.Checked = Autostart.IsEnabled;
        foreach (var (t, l) in new[] { (_sens, _sensL), (_size, _sizeL), (_dwell, _dwellL) }) { t.ValueChanged += (_, _) => Refresh_(); }
        Refresh_();

        FillTree(s);

        var grid = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 3, Padding = new Padding(14, 12, 14, 0) };
        grid.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        grid.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        grid.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        void Row(string label, Control c, Control extra = null)
        {
            int r = grid.RowCount++;
            grid.Controls.Add(new Label { Text = label, AutoSize = true, Anchor = AnchorStyles.Left, Margin = new Padding(0, 8, 14, 8) }, 0, r);
            grid.Controls.Add(c, 1, r);
            if (extra != null) grid.Controls.Add(extra, 2, r);
            c.Margin = new Padding(0, 4, 8, 4);
        }
        Row("Боковая кнопка мыши", _button);
        Row("Чувствительность", _sens, _sensL);
        Row("Размер меню", _size, _sizeL);
        Row("Задержка раскрытия", _dwell, _dwellL);
        var browse = new Button { Text = "Обзор…", AutoSize = true };
        browse.Click += (_, _) =>
        {
            using var d = new FolderBrowserDialog { Description = "Папка хранилища Obsidian", SelectedPath = _vault.Text, UseDescriptionForTitle = true };
            if (d.ShowDialog(this) == DialogResult.OK) _vault.Text = d.SelectedPath;
        };
        Row("Хранилище Obsidian", _vault, browse);

        var lower = new FlowLayoutPanel { Dock = DockStyle.Top, AutoSize = true, FlowDirection = FlowDirection.TopDown, Padding = new Padding(14, 4, 14, 0), WrapContents = false };
        lower.Controls.Add(_auto);
        lower.Controls.Add(new Label { Text = "Пункты меню (снимите галочку, чтобы убрать):", AutoSize = true, Margin = new Padding(3, 12, 3, 4) });
        lower.Controls.Add(_tree);

        var ok = new Button { Text = "Сохранить", Width = 110, DialogResult = DialogResult.None };
        var cancel = new Button { Text = "Отмена", Width = 90 };
        ok.Click += (_, _) => Save();
        cancel.Click += (_, _) => Close();
        var ver = new Label { Text = "Версия " + Cfg.Version, AutoSize = true, ForeColor = SystemColors.GrayText, Anchor = AnchorStyles.Left, Margin = new Padding(0, 8, 0, 0) };
        var bar = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 52, FlowDirection = FlowDirection.RightToLeft, Padding = new Padding(10, 10, 10, 0) };
        bar.Controls.Add(cancel);
        bar.Controls.Add(ok);
        bar.Controls.Add(ver);
        AcceptButton = ok; CancelButton = cancel;

        Controls.Add(lower);
        Controls.Add(grid);
        Controls.Add(bar);
        ClientSize = new Size(560, 520);
        FormClosed += (_, _) => { if (_current == this) _current = null; };
    }

    void Refresh_()
    {
        _sensL.Text = _sens.Value + "%";
        _sizeL.Text = _size.Value + "%";
        _dwellL.Text = _dwell.Value + " мс";
    }

    void FillTree(Settings s)
    {
        var off = new HashSet<string>(s.DisabledItems);
        _tree.BeginUpdate();
        foreach (var g in MenuModel.FullTree().Children)
        {
            var gn = new TreeNode(g.Label) { Tag = g.Id, Checked = !off.Contains(g.Id) };
            foreach (var c in g.Children) gn.Nodes.Add(new TreeNode(c.Label) { Tag = c.Id, Checked = !off.Contains(c.Id) });
            _tree.Nodes.Add(gn);
            gn.Expand();
        }
        _tree.EndUpdate();
        // группа выключена - дочерние тоже
        _tree.AfterCheck += (_, e) =>
        {
            if (e.Action == TreeViewAction.Unknown) return;
            if (e.Node.Parent == null) foreach (TreeNode c in e.Node.Nodes) c.Checked = e.Node.Checked;
            else if (e.Node.Checked) e.Node.Parent.Checked = true;
        };
    }

    void Save()
    {
        var n = Cfg.S.Clone();
        n.Button = _button.SelectedIndex == 1 ? 2 : 1;
        n.SensPercent = _sens.Value;
        n.SizePercent = _size.Value;
        n.DwellMs = _dwell.Value;
        n.Vault = _vault.Text.Trim();
        n.DisabledItems = new List<string>();
        foreach (TreeNode g in _tree.Nodes)
        {
            if (!g.Checked) n.DisabledItems.Add((string)g.Tag);
            foreach (TreeNode c in g.Nodes) if (!c.Checked) n.DisabledItems.Add((string)c.Tag);
        }
        if (_tree.Nodes.Cast<TreeNode>().All(g => !g.Checked))
        {
            MessageBox.Show(this, "Должен остаться хотя бы один пункт меню.", "Настройки", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        _onSave(n, _auto.Checked);
        Close();
    }
}
