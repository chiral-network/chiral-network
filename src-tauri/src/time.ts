/**
 * Formats a date into a human-readable "time ago" string.
 * @param date The date to format.
 * @returns A string like "5 minutes ago".
 */
export function timeAgo(date: Date): string {
	const seconds = Math.floor((new Date().getTime() - date.getTime()) / 1000);
	const intervals: { [key: string]: number } = {
		year: 31536000,
		month: 2592000,
		day: 86400,
		hour: 3600,
		minute: 60
	};

	for (const [unit, secondsInUnit] of Object.entries(intervals)) {
		const interval = Math.floor(seconds / secondsInUnit);
		if (interval >= 1) {
			return `${interval} ${unit}${interval === 1 ? '' : 's'} ago`;
		}
	}
	return `${Math.floor(seconds)} second${seconds === 1 ? '' : 's'} ago`;
}