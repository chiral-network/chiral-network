# Add French, Bengali, and Arabic Languages to Chiral Network

## Current Language Implementation

The application uses **svelte-i18n** for internationalization with 7 currently supported languages:

- English (en)
- Spanish (es)
- Chinese (zh)
- Korean (ko)
- Russian (ru)
- Portuguese (pt)
- Hindi (hi)

### Key Files Located

1. **[src/i18n/i18n.ts](src/i18n/i18n.ts)** - Main i18n configuration

   - Lines 8-14: Language registration using `register()` function
   - Lines 59-60: RTL language detection for right-to-left text direction
   - Handles locale detection, saving, and loading
   - Uses Tauri Store for persistence

2. **[src/locales/](src/locales/)** - Translation JSON files

   - `en.json`, `es.json`, `ko.json`, `ru.json`, `zh.json`, `pt.json`, `hi.json`
   - Each ~1400-1500 lines with identical structure
   - Contains all UI strings organized by feature

3. **[src/pages/Settings.svelte](src/pages/Settings.svelte)** - Language selector UI

   - Lines 173-181: `languages` array defining dropdown options (currently 7 languages)
   - Lines 1722-1741: Language Settings UI section
   - Lines 913-918: `onLanguageChange()` handler function

4. **[docs/i18n.md](docs/i18n.md)** - Developer documentation

## Implementation Steps to Add French (fr), Bengali (bn), and Arabic (ar)

### Step 1: Create New Translation Files

Create three new JSON files in `src/locales/`:

- `src/locales/fr.json` (French)
- `src/locales/bn.json` (Bengali)
- `src/locales/ar.json` (Arabic)

**Structure**: Copy `src/locales/en.json` as a template. Each file needs all translation keys (~1500 lines) translated to the target language.

**Critical keys to translate** (found in all locale files around lines 433-441):

```json
{
  "language.title": "Language",
  "language.select": "Select Language",
  "language.english": "English",
  "language.spanish": "Español",
  "language.chinese": "中文",
  "language.korean": "한국어",
  "language.russian": "Русский",
  "language.portuguese": "Português",
  "language.hindi": "हिन्दी",
  "language.french": "Français",      // NEW
  "language.bengali": "বাংলা",         // NEW
  "language.arabic": "العربية"         // NEW
}
```

### Step 2: Register New Languages in i18n Configuration

In **[src/i18n/i18n.ts](src/i18n/i18n.ts)**, add three new `register()` calls after line 14:

```typescript
register("en", () => import("../locales/en.json"));
register("ko", () => import("../locales/ko.json"));
register("es", () => import("../locales/es.json"));
register("zh", () => import("../locales/zh.json"));
register("ru", () => import("../locales/ru.json"));
register("pt", () => import("../locales/pt.json"));
register("hi", () => import("../locales/hi.json"));
register("fr", () => import("../locales/fr.json"));  // ADD THIS
register("bn", () => import("../locales/bn.json"));  // ADD THIS
register("ar", () => import("../locales/ar.json"));  // ADD THIS
```

**Important**: Update the RTL language set on lines 59-60 and 66-67 to include Arabic:

```typescript
// Line 59-60 in setupI18n function
const rtl = new Set(["ar", "he", "fa", "ur"]);
document.documentElement.setAttribute("dir", rtl.has(pick) ? "rtl" : "ltr");

// Line 66-67 in changeLocale function  
const rtl = new Set(["ar", "he", "fa", "ur"]);
document.documentElement.setAttribute("dir", rtl.has(lang) ? "rtl" : "ltr");
```

Note: Arabic (`ar`) is already included in the RTL set, so no changes needed here. The system will automatically apply right-to-left text direction when Arabic is selected.

### Step 3: Add Language Options to Settings UI

In **[src/pages/Settings.svelte](src/pages/Settings.svelte)**, update the `languages` array (lines 173-181):

```typescript
$: languages = [
  { value: "en", label: tr("language.english") },
  { value: "es", label: tr("language.spanish") },
  { value: "zh", label: tr("language.chinese") },
  { value: "ko", label: tr("language.korean") },
  { value: "ru", label: tr("language.russian") },
  { value: "pt", label: tr("language.portuguese") },
  { value: "hi", label: tr("language.hindi") },
  { value: "fr", label: tr("language.french") },     // ADD THIS
  { value: "bn", label: tr("language.bengali") },    // ADD THIS
  { value: "ar", label: tr("language.arabic") },     // ADD THIS
];
```

### Step 4: Update All Existing Locale Files

Add the new language name keys to **all 7 existing locale files**:

**In `en.json`, `es.json`, `ko.json`, `ru.json`, `zh.json`, `pt.json`, `hi.json`** (around lines 440-441):

Add these three new keys:
- `"language.french": "Français"`
- `"language.bengali": "বাংলা"`
- `"language.arabic": "العربية"`

Each file needs all three keys so users can see all language names regardless of their current language selection.

**Example for en.json:**
```json
"language.title": "Language",
"language.select": "Select Language",
"language.english": "English",
"language.spanish": "Español",
"language.chinese": "中文",
"language.korean": "한국어",
"language.russian": "Русский",
"language.portuguese": "Português",
"language.hindi": "हिन्दी",
"language.french": "Français",
"language.bengali": "বাংলা",
"language.arabic": "العربية",
```

### Step 5: Update Documentation

In **[docs/i18n.md](docs/i18n.md)** (lines 7-14), update the supported languages list:

```markdown
Currently supported languages:
- **English (en)** - Default
- **Spanish (es)**
- **Russian (ru)**
- **Chinese (zh)**
- **Korean (ko)**
- **Portuguese (pt)**
- **Hindi (hi)**
- **French (fr)** - NEW
- **Bengali (bn)** - NEW
- **Arabic (ar)** - NEW (RTL)
```

## Translation Strategy

For the ~1500 translation keys in `fr.json`, `bn.json`, and `ar.json`:

1. **Use Professional Translation Services** like DeepL or Google Translate for initial draft
2. **Review Key Sections Carefully**:

   - Navigation menu (`nav.*`)
   - Settings labels (`settings.*`)
   - Error messages (`errors.*`)
   - Action buttons (`actions.*`)

3. **Special Considerations for Arabic (RTL)**:
   - Test all UI layouts in right-to-left mode
   - Ensure icons and buttons are mirrored appropriately
   - Verify text alignment is correct
   - Check that bidirectional text (mixing Arabic with English/numbers) renders properly

4. **Bengali Script Considerations**:
   - Ensure proper font support for Bengali characters
   - Test conjunct consonants render correctly
   - Verify Unicode normalization for Bengali text

5. **Test with Native Speakers** before release
6. **Maintain Consistency** - use the same terms throughout

## Testing Checklist

After implementation:

- [ ] All three languages appear in Settings → Language dropdown
- [ ] Switching to French changes all UI text correctly
- [ ] Switching to Bengali changes all UI text correctly
- [ ] Switching to Arabic changes all UI text correctly
- [ ] **Arabic: RTL layout is properly applied (text flows right-to-left)**
- [ ] **Arabic: Icons and UI elements are mirrored appropriately**
- [ ] **Arabic: Bidirectional text (Arabic + English) renders correctly**
- [ ] Language preference persists after app restart
- [ ] No missing translations (no fallback to English keys)
- [ ] Special characters display correctly for all three languages
- [ ] Bengali conjunct consonants render properly
- [ ] Font rendering is clear and readable for all three languages

## Files Summary

**Files to create (3):**

- `src/locales/fr.json` (French)
- `src/locales/bn.json` (Bengali)
- `src/locales/ar.json` (Arabic)

**Files to modify (9):**

- `src/i18n/i18n.ts` (add 3 register calls - RTL already configured for Arabic)
- `src/pages/Settings.svelte` (add 3 array entries)
- `src/locales/en.json` (add 3 language keys)
- `src/locales/es.json` (add 3 language keys)
- `src/locales/ko.json` (add 3 language keys)
- `src/locales/ru.json` (add 3 language keys)
- `src/locales/zh.json` (add 3 language keys)
- `src/locales/pt.json` (add 3 language keys)
- `src/locales/hi.json` (add 3 language keys)
- `docs/i18n.md` (update documentation)

## Language Codes & Names Reference

| Language | ISO Code | Native Name | Script | Direction |
|----------|----------|-------------|--------|-----------|
| French   | fr       | Français    | Latin  | LTR       |
| Bengali  | bn       | বাংলা       | Bengali | LTR      |
| Arabic   | ar       | العربية     | Arabic | **RTL**   |

## Implementation To-dos

### Step 1: Create Translation Files
- [ ] Create `src/locales/fr.json` with complete French translations
- [ ] Create `src/locales/bn.json` with complete Bengali translations
- [ ] Create `src/locales/ar.json` with complete Arabic translations

### Step 2: Register Languages
- [ ] Add French registration to `src/i18n/i18n.ts`
- [ ] Add Bengali registration to `src/i18n/i18n.ts`
- [ ] Add Arabic registration to `src/i18n/i18n.ts`
- [ ] Verify RTL configuration includes Arabic (already done - no changes needed)

### Step 3: Update Settings UI
- [ ] Add French option to languages array in `src/pages/Settings.svelte`
- [ ] Add Bengali option to languages array in `src/pages/Settings.svelte`
- [ ] Add Arabic option to languages array in `src/pages/Settings.svelte`

### Step 4: Update Existing Locale Files
- [ ] Add 3 language keys to `src/locales/en.json`
- [ ] Add 3 language keys to `src/locales/es.json`
- [ ] Add 3 language keys to `src/locales/ko.json`
- [ ] Add 3 language keys to `src/locales/ru.json`
- [ ] Add 3 language keys to `src/locales/zh.json`
- [ ] Add 3 language keys to `src/locales/pt.json`
- [ ] Add 3 language keys to `src/locales/hi.json`

### Step 5: Update Documentation
- [ ] Update `docs/i18n.md` with French, Bengali, and Arabic

### Step 6: Testing
- [ ] Test French language switching and UI
- [ ] Test Bengali language switching and UI
- [ ] Test Arabic language switching and RTL layout
- [ ] Test language persistence across app restarts
- [ ] Verify all special characters render correctly

## Expected Outcome

After implementation, the Chiral Network application will support **10 languages**:

1. English (en)
2. Spanish (es)
3. Chinese (zh)
4. Korean (ko)
5. Russian (ru)
6. Portuguese (pt)
7. Hindi (hi)
8. **French (fr)** ✨ NEW
9. **Bengali (bn)** ✨ NEW
10. **Arabic (ar)** ✨ NEW (with RTL support)

This will make the application accessible to approximately **3.2 billion additional native speakers** worldwide:
- French: ~280 million speakers
- Bengali: ~265 million speakers
- Arabic: ~422 million speakers (with RTL UI support)

