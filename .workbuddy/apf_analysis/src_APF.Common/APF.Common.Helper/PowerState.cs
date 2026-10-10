using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace APF.Common.Helper;

[StructLayout(LayoutKind.Sequential)]
public class PowerState
{
	public ACLineStatus ACLineStatus;

	public BatteryFlag BatteryFlag;

	public byte BatteryLifePercent;

	public byte Reserved1;

	public int BatteryLifeTime;

	public int BatteryFullLifeTime;

	[MethodImpl(MethodImplOptions.NoInlining)]
	private PowerState()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static PowerState GetPowerState()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[DllImport("Kernel32", EntryPoint = "GetSystemPowerStatus")]
	private static extern bool GetSystemPowerStatusRef(PowerState sps);

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 椀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 樀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 欀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static PowerState()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
