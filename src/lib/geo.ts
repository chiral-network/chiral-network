export interface GeoRegionConfig {
  id: string;
  label: string;
  lat: number;
  lng: number;
  color: string;
  aliases: string[];
}

export const GEO_REGIONS: GeoRegionConfig[] = [
  {
    id: 'usEast',
    label: 'US-East',
    lat: 39.0,
    lng: -77.0,
    color: '#2563eb',
    aliases: ['useast', 'us-east', 'na-east', 'northamericaeast', 'us_east'],
  },
  {
    id: 'usWest',
    label: 'US-West',
    lat: 37.5,
    lng: -122.0,
    color: '#0891b2',
    aliases: ['uswest', 'us-west', 'na-west', 'northamericawest', 'us_west'],
  },
  {
    id: 'euWest',
    label: 'EU-West',
    lat: 50.0,
    lng: 2.0,
    color: '#16a34a',
    aliases: ['euwest', 'eu-west', 'europewest', 'eu_west'],
  },
  {
    id: 'euCentral',
    label: 'EU-Central',
    lat: 52.0,
    lng: 13.4,
    color: '#65a30d',
    aliases: ['eucentral', 'eu-central', 'europecentral', 'eu_central', 'eu'],
  },
  {
    id: 'asiaPacific',
    label: 'Asia-Pacific',
    lat: 1.35,
    lng: 103.8,
    color: '#f97316',
    aliases: ['asia-pacific', 'asiapacific', 'apac', 'asia', 'asia_pacific'],
  },
  {
    id: 'asiaEast',
    label: 'Asia-East',
    lat: 35.7,
    lng: 139.7,
    color: '#fb7185',
    aliases: ['asiaeast', 'asia-east', 'apac-east', 'asia_east', 'jp'],
  },
  {
    id: 'southAmerica',
    label: 'South America',
    lat: -23.5,
    lng: -46.6,
    color: '#d946ef',
    aliases: ['southamerica', 'south-america', 'latam', 'sa'],
  },
  {
    id: 'africa',
    label: 'Africa',
    lat: 9.1,
    lng: 21.3,
    color: '#facc15',
    aliases: ['africa', 'emea-africa', 'za', 'nigeria'],
  },
  {
    id: 'oceania',
    label: 'Oceania',
    lat: -33.8,
    lng: 151.2,
    color: '#38bdf8',
    aliases: ['oceania', 'australia', 'anz', 'apac-south', 'nz'],
  },
  {
    id: 'unknown',
    label: 'Unknown',
    lat: 0,
    lng: 0,
    color: '#9ca3af',
    aliases: ['unknown', 'unassigned', 'n/a', ''],
  },
];

const REGION_ALIAS_INDEX = new Map<string, GeoRegionConfig>();

for (const region of GEO_REGIONS) {
  REGION_ALIAS_INDEX.set(region.id.toLowerCase(), region);
  for (const alias of region.aliases) {
    REGION_ALIAS_INDEX.set(alias.toLowerCase(), region);
  }
}

export const UNKNOWN_REGION_ID = 'unknown';

export function normalizeRegion(value?: string | null): GeoRegionConfig {
  if (!value) {
    return REGION_ALIAS_INDEX.get(UNKNOWN_REGION_ID) as GeoRegionConfig;
  }

  const compact = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]/g, '');

  if (!compact) {
    return REGION_ALIAS_INDEX.get(UNKNOWN_REGION_ID) as GeoRegionConfig;
  }

  const explicit = REGION_ALIAS_INDEX.get(value.trim().toLowerCase());
  if (explicit) {
    return explicit;
  }

  const normalized = REGION_ALIAS_INDEX.get(compact);
  if (normalized) {
    return normalized;
  }

  return REGION_ALIAS_INDEX.get(UNKNOWN_REGION_ID) as GeoRegionConfig;
}

export function projectToMap(
  lat: number,
  lng: number,
  width: number,
  height: number
): { x: number; y: number } {
  const x = ((lng + 180) / 360) * width;
  const y = ((90 - lat) / 180) * height;
  return { x, y };
}
