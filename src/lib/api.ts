import axios from 'axios';

// Tauri app (master PC): always use localhost sidecar.
// Browser clients (LAN PCs): use the same host they navigated to — this
// automatically resolves to the master PC's IP (e.g. http://192.168.1.100:8000).
function _resolveApiUrl(): string {
  if (typeof window === 'undefined') return 'http://127.0.0.1:8000/api';
  const host = window.location.hostname;
  const port = window.location.port;
  // Vite dev server: use relative URL so the Vite proxy handles it (avoids CORS)
  if (port === '5173') return '/api';
  // Tauri webview always reports hostname as 'localhost', 'tauri.localhost', or
  // '127.0.0.1' — never a real LAN IP. Use this instead of window.__TAURI__
  // which may not be injected yet at module-load time.
  const isTauri = host === 'localhost' || host === 'tauri.localhost' || host === '127.0.0.1';
  if (isTauri) return 'http://127.0.0.1:8000/api';
  // Browser on a LAN client machine — same origin as the page
  return `${window.location.protocol}//${window.location.host}/api`;
}

const API_URL = _resolveApiUrl();

const api = axios.create({
  baseURL: API_URL,
  timeout: 30000, // 30 s — individual requests override this for large uploads (backup/restore)
  headers: {
    'Content-Type': 'application/json',
  },
});

api.interceptors.request.use(
  (config) => {
    // Only inject the user token if no Authorization header is already set.
    // This allows the admin panel to pass its own admin JWT without it being overwritten.
    if (!config.headers.Authorization) {
      const token = localStorage.getItem('cops_token');
      if (token) {
        config.headers.Authorization = `Bearer ${token}`;
      }
    }
    return config;
  },
  (error) => Promise.reject(error)
);

api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('cops_token');
      localStorage.removeItem('cops_user');
      window.dispatchEvent(new Event('auth_declined'));
    }
    return Promise.reject(error);
  }
);

export default api;
