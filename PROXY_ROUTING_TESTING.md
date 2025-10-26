# Testing Guide: Proxy Routing & Privacy Integration

## Quick Start
1. Build and run the app: `npm run tauri dev`
2. Navigate to the **Proxy** page in the sidebar
3. Follow the tests below

---

## **Unit Tests (Automated)**
All 15 tests for proxyRoutingService passed ✅

Run tests anytime:
```bash
npm run test -- tests/proxyRouting.test.ts
```

Tests cover:
- Proxy round-robin routing
- Weighted proxy selection
- Privacy settings tracking
- Route history management
- Statistics generation
- localStorage persistence

---

## **Manual UI Testing** (When App Runs)

### Test 1: Privacy Toggle - Anonymous Mode
```
1. Go to Proxy page
2. Look for "Privacy Settings" section
3. Click "Anonymous Mode" toggle
4. EXPECTED: Toggle switches ON (blue)
5. EXPECTED: "Current Privacy Mode" shows "Anonymous Mode"
6. Refresh page - EXPECTED: Toggle stays ON (persisted)
7. Click toggle again - EXPECTED: Turns OFF
```

### Test 2: Privacy Toggle - Multi-Hop Routing
```
1. Go to Proxy page
2. Click "Multi-Hop Routing" toggle
3. EXPECTED: Toggle switches ON (purple)
4. EXPECTED: "Current Privacy Mode" updates to show "Multi-Hop Routing"
5. Refresh page - EXPECTED: Toggle stays ON
```

### Test 3: Combined Privacy Modes
```
1. Enable both toggles (Anonymous Mode + Multi-Hop)
2. EXPECTED: "Current Privacy Mode" shows "Maximum Privacy (Anonymous + Multi-Hop)"
3. Disable one - EXPECTED: Status updates accordingly
4. Disable both - EXPECTED: Shows "Standard Mode"
5. Refresh page - EXPECTED: All settings persist
```

### Test 4: Proxy Routing Stats
```
1. Add at least 2 proxy nodes in the UI:
   - Click "Add Node"
   - Enter: proxy1.example.com:8080
   - Enter: proxy2.example.com:8080
2. Look at "Proxy Routing Stats" section
3. EXPECTED: "Available Proxies" shows count of added proxies
4. EXPECTED: "Total Routes" increases as proxies are cycled
5. EXPECTED: "Privacy Profile" displays current mode
```

### Test 5: localStorage Persistence
```
1. Toggle Anonymous Mode ON
2. Toggle Multi-Hop Routing ON
3. OPEN BROWSER DEV TOOLS (F12)
4. Go to Application > Local Storage > this domain
5. Look for:
   - "privacyProfile" should contain: {"anonymous":true,"multiHop":true}
   - "proxyRoutingConfig" should contain your config
6. Refresh page
7. EXPECTED: Both toggles still ON
8. EXPECTED: Privacy Mode still shows "Maximum Privacy"
```

### Test 6: UI Responsiveness
```
1. Toggle Anonymous Mode multiple times
2. EXPECTED: UI updates instantly (no lag)
3. EXPECTED: Privacy Profile text updates immediately
4. EXPECTED: Console shows no errors (F12 Console tab)
```

---

## **Integration Testing** (With Real Proxies)

When proxies are actually available:

```javascript
// Open browser console (F12) and test manually:

// Test 1: Get next route
import { proxyRoutingService } from '$lib/services/proxyRoutingService'
proxyRoutingService.getNextRoute()
// EXPECTED: Returns object with proxyAddress, anonymousMode, multiHopEnabled, timestamp

// Test 2: Get statistics
proxyRoutingService.getStatistics()
// EXPECTED: Shows totalRoutes count increasing

// Test 3: Check privacy profile
proxyRoutingService.getPrivacyProfile()
// EXPECTED: Returns current { anonymous, multiHop }
```

---

## **Browser DevTools Console Testing**

```javascript
// Paste these one at a time in browser console (F12 > Console):

// Test privacy store updates
import { privacyStore } from '$lib/services/proxyRoutingService'
privacyStore.subscribe(value => console.log('Privacy updated:', value))

// Toggle in UI and watch console - should log updates

// Test route generation
import { proxyRoutingService } from '$lib/services/proxyRoutingService'
for(let i = 0; i < 5; i++) {
  console.log(`Route ${i}:`, proxyRoutingService.getNextRoute())
}
// EXPECTED: Each route should have different/cycling proxy addresses
```

---

## **What to Look For**

### ✅ Correct Behavior
- Toggles switch colors smoothly
- Text updates instantly when toggle changes
- Refresh page = settings persist
- Multiple toggles work independently
- Stats update as you add proxies
- Console has no errors

### ❌ Issues to Report
- Toggle doesn't switch
- Privacy Profile text doesn't update
- Settings lost after refresh
- Console shows errors
- UI is slow/laggy
- localStorage not persisting

---

## **Files to Test**
- **Service**: `src/lib/services/proxyRoutingService.ts` ✅ (tested)
- **UI**: `src/pages/Proxy.svelte` (manual testing needed)
- **Tests**: `tests/proxyRouting.test.ts` ✅ (all 15 passing)

---

## **Next Steps**
1. ✅ Run unit tests: `npm run test -- tests/proxyRouting.test.ts`
2. ⏳ Manual UI testing (when app builds)
3. ⏳ Integration testing with actual proxies
