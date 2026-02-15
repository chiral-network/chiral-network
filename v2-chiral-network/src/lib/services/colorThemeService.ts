import type { ColorTheme } from '$lib/stores';

// Each theme maps shade -> space-separated RGB values (for Tailwind opacity support)
// Values from Tailwind CSS default color palette
const themeColors: Record<ColorTheme, Record<number, string>> = {
  blue: {
    50:  '239 246 255',
    100: '219 234 254',
    200: '191 219 254',
    300: '147 197 253',
    400: '96 165 250',
    500: '59 130 246',
    600: '37 99 235',
    700: '29 78 216',
    800: '30 64 175',
    900: '30 58 138',
    950: '23 37 84',
  },
  purple: {
    50:  '250 245 255',
    100: '243 232 255',
    200: '233 213 255',
    300: '216 180 254',
    400: '192 132 252',
    500: '168 85 247',
    600: '147 51 234',
    700: '126 34 206',
    800: '107 33 168',
    900: '88 28 135',
    950: '59 7 100',
  },
  green: {
    50:  '240 253 244',
    100: '220 252 231',
    200: '187 247 208',
    300: '134 239 172',
    400: '74 222 128',
    500: '34 197 94',
    600: '22 163 74',
    700: '21 128 61',
    800: '22 101 52',
    900: '20 83 45',
    950: '5 46 22',
  },
  red: {
    50:  '254 242 242',
    100: '254 226 226',
    200: '254 202 202',
    300: '252 165 165',
    400: '248 113 113',
    500: '239 68 68',
    600: '220 38 38',
    700: '185 28 28',
    800: '153 27 27',
    900: '127 29 29',
    950: '69 10 10',
  },
  orange: {
    50:  '255 247 237',
    100: '255 237 213',
    200: '254 215 170',
    300: '253 186 116',
    400: '251 146 60',
    500: '249 115 22',
    600: '234 88 12',
    700: '194 65 12',
    800: '154 52 18',
    900: '124 45 18',
    950: '67 20 7',
  },
};

const shades = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;

export function applyColorTheme(theme: ColorTheme): void {
  const colors = themeColors[theme];
  if (!colors) return;
  const root = document.documentElement;
  for (const shade of shades) {
    root.style.setProperty(`--color-primary-${shade}`, colors[shade]);
  }
}

export const availableThemes: { value: ColorTheme; label: string; previewHex: string }[] = [
  { value: 'blue',   label: 'Blue',   previewHex: '#3b82f6' },
  { value: 'purple', label: 'Purple', previewHex: '#a855f7' },
  { value: 'green',  label: 'Green',  previewHex: '#22c55e' },
  { value: 'red',    label: 'Red',    previewHex: '#ef4444' },
  { value: 'orange', label: 'Orange', previewHex: '#f97316' },
];
