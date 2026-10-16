using System.Diagnostics;
using System.Runtime.InteropServices;

namespace RadialMenu;

/// <summary>Следит за пиком звука на устройстве вывода: Playing = true, если недавно был звук.</summary>
internal sealed class MediaState
{
    const float Threshold = 0.002f;   // пик 0..1
    const double HoldSec = 1.5;       // пауза между треками не считается выключением

    public volatile bool Playing;
    double _lastSound = -100;
    readonly Stopwatch _clock = Stopwatch.StartNew();

    public void Start()
    {
        var t = new Thread(Run) { IsBackground = true, Name = "MediaState" };
        t.SetApartmentState(ApartmentState.MTA);
        t.Start();
    }

    void Run()
    {
        IAudioMeterInformation meter = null;
        double opened = -100;
        while (true)
        {
            try
            {
                double now = _clock.Elapsed.TotalSeconds;
                if (meter == null || now - opened > 10)   // устройство вывода могли сменить
                {
                    Release(meter);
                    meter = Open();
                    opened = now;
                }
                if (meter != null)
                {
                    if (meter.GetPeakValue(out float peak) != 0) { Release(meter); meter = null; }
                    else if (peak > Threshold) _lastSound = now;
                }
                Playing = now - _lastSound < HoldSec;
            }
            catch (Exception)
            {
                Release(meter);
                meter = null;
            }
            Thread.Sleep(100);
        }
    }

    static void Release(object o)
    {
        try { if (o != null) Marshal.ReleaseComObject(o); } catch { }
    }

    static IAudioMeterInformation Open()
    {
        var enumType = Type.GetTypeFromCLSID(new Guid("BCDE0395-E52F-467C-8E3D-C4579291692E"));
        var en = (IMMDeviceEnumerator)Activator.CreateInstance(enumType);
        try
        {
            if (en.GetDefaultAudioEndpoint(0, 0, out IMMDevice dev) != 0) return null;
            try
            {
                var iid = new Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064");
                if (dev.Activate(ref iid, 23, IntPtr.Zero, out object o) != 0) return null;
                return (IAudioMeterInformation)o;
            }
            finally { Release(dev); }
        }
        finally { Release(en); }
    }

    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IMMDeviceEnumerator
    {
        [PreserveSig] int EnumAudioEndpoints(int dataFlow, int stateMask, out IntPtr devices);
        [PreserveSig] int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice device);
    }

    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IMMDevice
    {
        [PreserveSig] int Activate(ref Guid iid, int clsCtx, IntPtr activationParams,
            [MarshalAs(UnmanagedType.IUnknown)] out object iface);
    }

    [ComImport, Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IAudioMeterInformation
    {
        [PreserveSig] int GetPeakValue(out float peak);
    }
}
