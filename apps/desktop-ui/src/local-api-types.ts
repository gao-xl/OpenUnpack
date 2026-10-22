export type ApiScope = 'jobs.read' | 'jobs.write' | 'files.read' | 'files.write' | 'files.delete' | 'vault.read' | 'vault.write' | 'vault.reveal' | 'settings.read' | 'settings.write' | 'desktop.interactive';
export interface LocalApiSettings { enabled: boolean; port: number }
export interface ApiClientInput { name: string; scopes: ApiScope[]; readRoots: string[]; writeRoots: string[]; expiresAt?: string | null }
export interface ApiClientView extends ApiClientInput { id: string; createdAt: string; lastUsedAt?: string | null }
// Only the create response carries the one-time token. Status/list views never do.
export interface CreatedApiClient { client: ApiClientView; token: string }
export interface ApiPermission { scope: ApiScope; label: string; sensitive: boolean }
export interface LocalApiStatus {
  settings: LocalApiSettings; running: boolean; baseUrl: string | null; docsUrl: string | null; error: string | null;
  clients: ApiClientView[]; permissions: ApiPermission[];
}
