# macOS Power Monitoring Implementation

## Overview

The Mining tab in Chiral Network now displays real-time power consumption data on macOS systems. This feature uses hardware readings from the System Management Controller (SMC) when available, with an intelligent fallback to CPU usage-based estimation.

## How It Works

### Primary Method: SMC Hardware Readings

The implementation attempts to read power consumption directly from the Mac's System Management Controller (SMC), which provides accurate hardware power metrics:

- **PCPC** (Package CPU Power): Total CPU package power
- **PSTR** (Power System Total): System-wide power consumption
- **PC0C** (CPU Core 0 Power): Individual core power
- **PCTR** (CPU Total Residency): CPU power residency

These values are read in real-time and displayed in the Mining tab's Power Usage card.

### Fallback Method: CPU Usage Estimation

If SMC access is unavailable (due to permissions or hardware limitations), the system falls back to estimating power consumption based on:

1. **CPU Model Detection**: Identifies whether you're using:
   - Apple Silicon (M1, M2, M3) - ~20W TDP
   - Intel i9 - ~45W TDP
   - Intel i7 - ~28W TDP
   - Intel i5 - ~20W TDP
   - Other models - ~15W TDP (default)

2. **Real-time CPU Usage**: Measures actual CPU utilization using system tools

3. **Power Calculation**: Combines TDP with usage percentage plus base system power

## Testing the Feature

### What You'll See

In the Mining tab, the "Power Usage" card will display:

- **Real power value** (e.g., "45W") when SMC or estimation is working
- **"N/A"** if power monitoring is unavailable
- **Efficiency metric** (H/W) showing hash rate per watt when mining

### Expected Behavior

#### Apple Silicon Macs (M1/M2/M3)
- Lower power consumption (typically 10-30W during mining)
- Very efficient hash-per-watt ratio
- SMC readings may vary by model

#### Intel Macs
- Higher power consumption (typically 30-80W during mining)
- Power scales with CPU usage
- Older models may show higher values

### Verifying It Works

1. **Start the application**: Open Chiral Network
2. **Navigate to Mining tab**: Click "Mining" in the sidebar
3. **Check Power Usage card**: Look for the power value in watts
4. **Start mining**: Click "Start Mining" to see power increase
5. **Verify changes**: Power consumption should change based on mining intensity

### Troubleshooting

**Power shows "N/A"**
- SMC access may be restricted
- Try running with different permissions
- Fallback estimation should work but may show N/A if system tools are unavailable

**Power seems incorrect**
- Initial readings may be inaccurate; wait a few seconds for stabilization
- The system uses weighted averaging to smooth out spikes
- CPU-based estimates are approximations, not exact measurements

**Power doesn't change when mining starts**
- This is expected if SMC access is working correctly
- SMC shows total system power, not just mining-related power
- Try the CPU usage fallback method for mining-specific estimates

## Technical Details

### Dependencies

- **smc crate v0.2.4**: Provides SMC access on macOS
- Platform-specific, only compiled for macOS targets

### Implementation Location

- `src-tauri/src/main.rs`: Power monitoring functions
  - `get_mac_power()`: Main entry point
  - `get_mac_power_from_smc()`: SMC hardware readings
  - `get_mac_power_from_cpu_usage()`: CPU-based estimation

### Permissions

No special permissions are required. If SMC access fails, the system automatically falls back to CPU-based estimation.

## Contributing

Found an issue or have improvements for Mac power monitoring?

1. Test the feature on your Mac
2. Note your Mac model and macOS version
3. Report findings in GitHub Issues
4. Submit pull requests with improvements

### Known Limitations

- SMC keys may vary between Mac models
- Some Mac models may not expose all power metrics
- CPU-based estimation is approximate
- Power readings are for the entire system, not just the mining process

## References

- [System Management Controller (SMC)](https://support.apple.com/en-us/HT201295)
- [smc crate documentation](https://docs.rs/smc/)
- [Energy Impact API](https://developer.apple.com/documentation/foundation/processinfo/thermalstate)

---

**Note**: Power consumption monitoring is for informational purposes only. Actual power usage may vary based on system configuration, background processes, and hardware efficiency.
