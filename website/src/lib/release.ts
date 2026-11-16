export const REPO = 'https://github.com/Shawiizz/prisme';
export const RELEASES = `${REPO}/releases/latest`;

export interface Release {
	version: string | null;
	windows: string;
	macos: string;
	deb: string;
	appImage: string;
	/** The newest Android package: phones have releases of their own (mobile-1.2.3). */
	android: string;
}

interface GitHubRelease {
	tag_name: string;
	draft: boolean;
	prerelease: boolean;
	assets: { name: string; browser_download_url: string }[];
}

let cached: Promise<Release> | undefined;

/** The latest packages, read once per build; the releases page when GitHub does not answer. */
export function latestRelease(): Promise<Release> {
	cached ??= fetchRelease();
	return cached;
}

const github = (path: string) =>
	fetch(`https://api.github.com/repos/Shawiizz/prisme/${path}`, {
		// in CI the workflow's token lifts the limit GitHub puts on anonymous calls
		headers: {
			Accept: 'application/vnd.github+json',
			...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}),
		},
		signal: AbortSignal.timeout(5000),
	});

async function fetchRelease(): Promise<Release> {
	const fallback: Release = {
		version: null,
		windows: RELEASES,
		macos: RELEASES,
		deb: RELEASES,
		appImage: RELEASES,
		android: `${REPO}/releases`,
	};
	try {
		const [latest, all] = await Promise.all([github('releases/latest'), github('releases?per_page=30')]);
		if (!latest.ok) return fallback;
		const release = (await latest.json()) as GitHubRelease;
		const asset = (from: GitHubRelease, suffix: string) =>
			from.assets.find((a) => a.name.endsWith(suffix))?.browser_download_url;
		const releases = all.ok ? ((await all.json()) as GitHubRelease[]) : [];
		const phone = releases.find((r) => !r.draft && !r.prerelease && asset(r, '.apk'));
		return {
			version: release.tag_name,
			windows: asset(release, '-setup.exe') ?? RELEASES,
			macos: asset(release, '.dmg') ?? RELEASES,
			deb: asset(release, '.deb') ?? RELEASES,
			appImage: asset(release, '.AppImage') ?? RELEASES,
			android: (phone && asset(phone, '.apk')) ?? fallback.android,
		};
	} catch {
		return fallback;
	}
}
