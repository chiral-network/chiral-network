import { GEO_REGIONS, UNKNOWN_REGION_ID, type GeoRegionConfig } from '$lib/geo';

export type GeolocationSource = 'browser' | 'timezone' | 'fallback';

export interface GeolocationResult {
  region: GeoRegionConfig;
  source: GeolocationSource;
}

const REGION_CANDIDATES = GEO_REGIONS.filter((region) => region.id !== UNKNOWN_REGION_ID);

const FALLBACK_REGION = REGION_CANDIDATES.find((region) => region.id === 'usEast') ?? REGION_CANDIDATES[0];

function toRadians(value: number): number {
  return (value * Math.PI) / 180;
}

function haversineDistance(lat1: number, lng1: number, lat2: number, lng2: number): number {
  const R = 6371; // Radius of Earth in km
  const dLat = toRadians(lat2 - lat1);
  const dLng = toRadians(lng2 - lng1);
  const a =
    Math.sin(dLat / 2) * Math.sin(dLat / 2) +
    Math.cos(toRadians(lat1)) *
      Math.cos(toRadians(lat2)) *
      Math.sin(dLng / 2) *
      Math.sin(dLng / 2);
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return R * c;
}

function nearestRegion(lat: number, lng: number): GeoRegionConfig {
  let closest = FALLBACK_REGION;
  let minDistance = Number.POSITIVE_INFINITY;

  for (const region of REGION_CANDIDATES) {
    const distance = haversineDistance(lat, lng, region.lat, region.lng);
    if (distance < minDistance) {
      minDistance = distance;
      closest = region;
    }
  }

  return closest;
}

function inferRegionFromTimezone(timezone: string): GeoRegionConfig | null {
  const tz = timezone.toLowerCase();

  const matchers: Array<{ test: (tz: string) => boolean; regionId: string }> = [
    {
      test: (value) =>
        /america\/(los_angeles|vancouver|whitehorse|sitka|anchorage|metlakatla|juneau|yakutat|tijuana|phoenix|boise|denver|edmonton|dawson|hermosillo|mazatlan)/.test(
          value
        ),
      regionId: 'usWest',
    },
    {
      test: (value) => /america\//.test(value),
      regionId: 'usEast',
    },
    {
      test: (value) => /europe\//.test(value),
      regionId: 'euWest',
    },
    {
      test: (value) => /(asia|indian)\//.test(value),
      regionId: 'asiaPacific',
    },
    {
      test: (value) => /(australia|pacific)\//.test(value),
      regionId: 'oceania',
    },
    {
      test: (value) => /africa\//.test(value),
      regionId: 'africa',
    },
    {
      test: (value) => /(america|south_america)\/(argentina|buenos_aires|santiago|sao_paulo|bogota|lima|la_paz|montevideo)/.test(value),
      regionId: 'southAmerica',
    },
  ];

  for (const matcher of matchers) {
    if (matcher.test(tz)) {
      const region = REGION_CANDIDATES.find((item) => item.id === matcher.regionId);
      if (region) {
        return region;
      }
    }
  }

  return null;
}

async function detectFromBrowserGeolocation(): Promise<GeoRegionConfig | null> {
  if (typeof window === 'undefined' || !('geolocation' in navigator)) {
    return null;
  }

  try {
    const position = await new Promise<GeolocationPosition>((resolve, reject) => {
      navigator.geolocation.getCurrentPosition(resolve, reject, {
        enableHighAccuracy: false,
        maximumAge: 60_000,
        timeout: 5_000,
      });
    });

    const { latitude, longitude } = position.coords;
    return nearestRegion(latitude, longitude);
  } catch (error) {
    return null;
  }
}

function detectFromTimezone(): GeoRegionConfig | null {
  if (typeof Intl === 'undefined' || typeof Intl.DateTimeFormat === 'undefined') {
    return null;
  }

  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  if (!timezone) {
    return null;
  }

  return inferRegionFromTimezone(timezone);
}

export async function detectUserRegion(): Promise<GeolocationResult> {
  const browserRegion = await detectFromBrowserGeolocation();
  if (browserRegion) {
    return { region: browserRegion, source: 'browser' };
  }

  const timezoneRegion = detectFromTimezone();
  if (timezoneRegion) {
    return { region: timezoneRegion, source: 'timezone' };
  }

  return { region: FALLBACK_REGION, source: 'fallback' };
}

export function calculateRegionDistance(a: GeoRegionConfig, b: GeoRegionConfig): number {
  return haversineDistance(a.lat, a.lng, b.lat, b.lng);
}