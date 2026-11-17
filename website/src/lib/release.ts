export const REPO = 'https://github.com/Shawiizz/prisme';
export const RELEASES = `${REPO}/releases/latest`;

export interface Release {
	version: string | null;
	windows: string;
	macos: string;
	deb: string;
	appImage: string;
	android: string;
}

interface GitHubRelease {
	tag_name: string;
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
		android: RELEASES,
	};
	try {
		const latest = await github('releases/latest');
		if (!latest.ok) return fallback;
		const release = (await latest.json()) as GitHubRelease;
		const asset = (from: GitHubRelease, suffix: string) =>
			from.assets.find((a) => a.name.endsWith(suffix))?.browser_download_url;
		return {
			version: release.tag_name,
			windows: asset(release, '-installer.exe') ?? RELEASES,
			macos: asset(release, '.dmg') ?? RELEASES,
			deb: asset(release, '.deb') ?? RELEASES,
			appImage: asset(release, '.AppImage') ?? RELEASES,
			android: asset(release, '.apk') ?? RELEASES,
		};
	} catch {
		return fallback;
	}
}
