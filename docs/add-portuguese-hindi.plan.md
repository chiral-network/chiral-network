<!-- 74a0d83e-1c37-48ee-8fe2-3ed81540b4e8 3c3f59db-805b-4d77-a103-3d3e6e223c6b -->
# Add Portuguese and Hindi Languages to Chiral Network

## Current Language Implementation

The application uses **svelte-i18n** for internationalization with 5 currently supported languages:

- English (en)
- Spanish (es)
- Chinese (zh)
- Korean (ko)
- Russian (ru)

### Key Files Located

1. **[src/i18n/i18n.ts](src/i18n/i18n.ts)** - Main i18n configuration

   - Lines 8-12: Language registration using `register()` function
   - Handles locale detection, saving, and loading
   - Uses Tauri Store for persistence

2. **[src/locales/](src/locales/)** - Translation JSON files

   - `en.json`, `es.json`, `ko.json`, `ru.json`, `zh.json`
   - Each ~1400-1500 lines with identical structure
   - Contains all UI strings organized by feature

3. **[src/pages/Settings.svelte](src/pages/Settings.svelte)** - Language selector UI

   - Lines 173-179: `languages` array defining dropdown options
   - Lines 1722-1741: Language Settings UI section
   - Lines 913-918: `onLanguageChange()` handler function

4. **[docs/i18n.md](docs/i18n.md)** - Developer documentation

## Implementation Steps to Add Portuguese (pt) and Hindi (hi)

### Step 1: Create New Translation Files

Create two new JSON files in `src/locales/`:

- `src/locales/pt.json` (Portuguese)
- `src/locales/hi.json` (Hindi)

**Structure**: Copy `src/locales/en.json` as a template. Each file needs all translation keys (~1500 lines) translated to the target language.

**Critical keys to translate** (found in all locale files around lines 303-312):

```json
{
  "language.title": "Language",
  "language.select": "Select Language",
  "language.english": "English",
  "language.spanish": "Español",
  "language.chinese": "中文",
  "language.korean": "한국어",
  "language.russian": "Русский",
  "language.portuguese": "Português",  // NEW
  "language.hindi": "हिन्दी"            // NEW
}
```

### Step 2: Register New Languages in i18n Configuration

In **[src/i18n/i18n.ts](src/i18n/i18n.ts)**, add two new `register()` calls after line 12:

```typescript
register("en", () => import("../locales/en.json"));
register("ko", () => import("../locales/ko.json"));
register("es", () => import("../locales/es.json"));
register("zh", () => import("../locales/zh.json"));
register("ru", () => import("../locales/ru.json"));
register("pt", () => import("../locales/pt.json"));  // ADD THIS
register("hi", () => import("../locales/hi.json"));  // ADD THIS
```

### Step 3: Add Language Options to Settings UI

In **[src/pages/Settings.svelte](src/pages/Settings.svelte)**, update the `languages` array (lines 173-179):

```typescript
$: languages = [
  { value: "en", label: tr("language.english") },
  { value: "es", label: tr("language.spanish") },
  { value: "zh", label: tr("language.chinese") },
  { value: "ko", label: tr("language.korean") },
  { value: "ru", label: tr("language.russian") },
  { value: "pt", label: tr("language.portuguese") },  // ADD THIS
  { value: "hi", label: tr("language.hindi") },       // ADD THIS
];
```

### Step 4: Update All Existing Locale Files

Add the new language name keys to **all 5 existing locale files**:

**In `en.json`, `es.json`, `ko.json`, `ru.json`, `zh.json`** (around line 310):

- Add: `"language.portuguese": "Português"`
- Add: `"language.hindi": "हिन्दी"`

Each file needs both keys so users can see all language names regardless of their current language selection.

### Step 5: Update Documentation

In **[docs/i18n.md](docs/i18n.md)** (lines 7-12), update the supported languages list:

```markdown
Currently supported languages:
- **English (en)** - Default
- **Spanish (es)**
- **Russian (ru)**
- **Chinese (zh)**
- **Korean (ko)**
- **Portuguese (pt)** - NEW
- **Hindi (hi)** - NEW
```

## Translation Strategy

For the ~1500 translation keys in `pt.json` and `hi.json`:

1. **Use Professional Translation Services** like DeepL or Google Translate for initial draft
2. **Review Key Sections Carefully**:

   - Navigation menu (`nav.*`)
   - Settings labels (`settings.*`)
   - Error messages (`errors.*`)
   - Action buttons (`actions.*`)

3. **Test with Native Speakers** before release
4. **Maintain Consistency** - use the same terms throughout

## Testing Checklist

After implementation:

- [x] Both languages appear in Settings → Language dropdown
- [x] Switching to Portuguese changes all UI text
- [x] Switching to Hindi changes all UI text  
- [x] Language preference persists after app restart
- [x] No missing translations (no fallback to English keys)
- [x] Special characters display correctly
- [ ] RTL layout works correctly for Hindi (if needed)

## Files Summary

**Files to create (2):**

- `src/locales/pt.json`
- `src/locales/hi.json`

**Files to modify (7):**

- `src/i18n/i18n.ts` (add 2 register calls)
- `src/pages/Settings.svelte` (add 2 array entries)
- `src/locales/en.json` (add 2 language keys)
- `src/locales/es.json` (add 2 language keys)
- `src/locales/ko.json` (add 2 language keys)
- `src/locales/ru.json` (add 2 language keys)
- `src/locales/zh.json` (add 2 language keys)
- `docs/i18n.md` (update documentation)

### To-dos

- [x] Create pt.json and hi.json in src/locales/ by copying en.json structure and translating all ~1500 keys
- [x] Add register() calls for Portuguese and Hindi in src/i18n/i18n.ts
- [x] Add Portuguese and Hindi options to languages array in src/pages/Settings.svelte
- [x] Add language.portuguese and language.hindi keys to all 5 existing locale JSON files
- [x] Update docs/i18n.md to include Portuguese and Hindi in supported languages list
- [x] Test language switching, persistence, and verify no missing translations

## Implementation Complete! ✅

All steps have been successfully implemented. The Chiral Network application now supports 7 languages:
1. English (en)
2. Spanish (es)
3. Chinese (zh)
4. Korean (ko)
5. Russian (ru)
6. Portuguese (pt) ✨
7. Hindi (hi) ✨

