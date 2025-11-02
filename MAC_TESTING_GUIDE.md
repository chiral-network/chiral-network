# Mac Power Monitoring - Quick Test Guide

## For the Mac User Testing This Feature

Hi! Thanks for testing the new Mac power monitoring feature. Here's what you need to know:

### What Changed

The "Power Usage" card in the Mining tab will now show **real power consumption values** instead of "N/A" for Mac users.

### How to Test

1. **Open Chiral Network** on your Mac
2. **Go to the Mining tab**
3. **Look at the "Power Usage" card** (top right area)
4. You should see one of:
   - A power value in watts (e.g., "25W", "45W") ✅ **This is working!**
   - "N/A" ❌ **Not working - please report details**

### What to Expect

**When NOT Mining:**
- Should show base system power (typically 10-30W for Apple Silicon, 30-60W for Intel Macs)

**When Mining:**
- Power should be higher, reflecting CPU usage
- Should increase with mining intensity/threads

### Test Checklist

- [ ] Open the app - does Power Usage show a number?
- [ ] Start mining - does power increase?
- [ ] Stop mining - does power decrease?
- [ ] Change mining intensity - does power change accordingly?

### What to Report

If you test this feature, please share:

1. **Mac Model**: (e.g., "MacBook Pro M1 2021", "iMac Intel i7 2019")
2. **macOS Version**: (e.g., "macOS Sonoma 14.2")
3. **Power Display**: What does it show? (exact value or "N/A")
4. **Mining Test Results**: Did power change when mining?
5. **Any Console Errors**: Check Developer Tools → Console

### Example Report

```
Mac Model: MacBook Pro M2 2023
macOS Version: macOS Sonoma 14.5
Power Display: Shows "18W" when idle
Mining Test: Increases to "35W" when mining with 4 threads
Status: ✅ Working perfectly!
```

### Where to Report

- GitHub Issue: [Link to issue once created]
- Pull Request Comments: Comment on this PR
- Zulip Chat: [Project chat if available]

### Known Limitations

- Some Mac models may not support SMC power readings
- Power is for the WHOLE system, not just mining
- Initial values may take a few seconds to stabilize
- Estimates are approximate, not exact measurements

### Troubleshooting

**"N/A" displayed:**
- This is expected if SMC access fails
- The fallback CPU estimation should work
- If both fail, please report your Mac model

**Unrealistic values:**
- Values > 200W might indicate an error
- Values < 5W are probably wrong
- Report these with your Mac specs

---

Thank you for helping test this feature! Your feedback is valuable for improving Mac support. 🙏

**Built by:** @copilot for Mac users
**Issue:** https://github.com/potato-weijie-li/chiral-network/issues/[number]
