// Color + Animal alias generator for ChiralDrop
// Generates a random alias that changes each session

const COLORS = [
  'Red', 'Blue', 'Green', 'Yellow', 'Orange', 'Purple', 'Pink', 'Cyan',
  'Magenta', 'Lime', 'Teal', 'Indigo', 'Coral', 'Crimson', 'Gold', 'Silver',
  'Amber', 'Azure', 'Jade', 'Ruby', 'Sapphire', 'Emerald', 'Violet', 'Scarlet'
];

const ANIMALS = [
  'Fox', 'Wolf', 'Bear', 'Eagle', 'Hawk', 'Owl', 'Tiger', 'Lion',
  'Panther', 'Falcon', 'Raven', 'Phoenix', 'Dragon', 'Serpent', 'Shark', 'Whale',
  'Dolphin', 'Orca', 'Lynx', 'Leopard', 'Jaguar', 'Puma', 'Cobra', 'Viper'
];

// Color codes for UI display
export const ALIAS_COLORS: Record<string, string> = {
  'Red': '#ef4444',
  'Blue': '#3b82f6',
  'Green': '#22c55e',
  'Yellow': '#eab308',
  'Orange': '#f97316',
  'Purple': '#a855f7',
  'Pink': '#ec4899',
  'Cyan': '#06b6d4',
  'Magenta': '#d946ef',
  'Lime': '#84cc16',
  'Teal': '#14b8a6',
  'Indigo': '#6366f1',
  'Coral': '#f87171',
  'Crimson': '#dc2626',
  'Gold': '#fbbf24',
  'Silver': '#94a3b8',
  'Amber': '#f59e0b',
  'Azure': '#0ea5e9',
  'Jade': '#10b981',
  'Ruby': '#e11d48',
  'Sapphire': '#2563eb',
  'Emerald': '#059669',
  'Violet': '#8b5cf6',
  'Scarlet': '#b91c1c'
};

export interface UserAlias {
  color: string;
  animal: string;
  displayName: string;
  colorHex: string;
}

function getRandomElement<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

export function generateAlias(): UserAlias {
  const color = getRandomElement(COLORS);
  const animal = getRandomElement(ANIMALS);

  return {
    color,
    animal,
    displayName: `${color} ${animal}`,
    colorHex: ALIAS_COLORS[color] || '#6b7280'
  };
}

// Generate a consistent alias from a peer ID (for displaying other users)
export function aliasFromPeerId(peerId: string): UserAlias {
  // Use peer ID hash to get consistent color/animal combo
  let hash = 0;
  for (let i = 0; i < peerId.length; i++) {
    const char = peerId.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32-bit integer
  }

  const colorIndex = Math.abs(hash) % COLORS.length;
  const animalIndex = Math.abs(hash >> 8) % ANIMALS.length;

  const color = COLORS[colorIndex];
  const animal = ANIMALS[animalIndex];

  return {
    color,
    animal,
    displayName: `${color} ${animal}`,
    colorHex: ALIAS_COLORS[color] || '#6b7280'
  };
}
