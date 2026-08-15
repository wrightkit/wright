/**
 * Resolves the absolute path to the native Wright binary (`wright` or `wright-lsp`) for the current platform.
 *
 * @param binName - Name of the binary to resolve ('wright' or 'wright-lsp', defaults to 'wright')
 * @returns The absolute filesystem path to the native executable
 * @throws Error if the current platform is unsupported or the platform package is not installed
 */
export function getBinaryPath(binName?: 'wright' | 'wright-lsp' | string): string;

/**
 * Returns the expected npm platform package name for the current platform/arch (e.g. '@wrightkit/wright-darwin-arm64'),
 * or null if unsupported.
 */
export function getPlatformPackageName(): string | null;

/**
 * Returns the current platform key (e.g. 'darwin-arm64', 'linux-x64', 'win32-x64').
 */
export function getPlatformKey(): string;

/**
 * Mapping of platform keys to package names.
 */
export declare const PLATFORMS: Record<string, string>;
