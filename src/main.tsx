import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import { ReactQueryDevtools } from '@tanstack/react-query-devtools'
import { queryClient } from './lib/queryClient'
import App from './App.tsx'
import './index.css'

// Show the Tauri window once the webview is ready.
// The window starts hidden (visible: false in tauri.conf.json) to prevent the
// split-second DWM double-surface flicker on Windows 11 (white frame visible
// before WebView2 paints the React UI).
//
// We call our custom Rust command `show_main_window` instead of window.show()
// directly. On Windows the command uses SW_SHOWNOACTIVATE so COPS appears in
// the taskbar WITHOUT stealing focus from Chrome or other apps. On Linux/macOS
// it falls back to the normal show() call.
//
// The retry loop handles Linux/WebKit2GTK where __TAURI_INTERNALS__ is injected
// asynchronously (not yet present when this module first runs). On Windows
// (WebView2) the global is available synchronously so the first attempt at
// t=50ms always succeeds.
if (typeof window !== 'undefined') {
  const _tryShowWindow = (attempt: number) => {
    if ('__TAURI_INTERNALS__' in window) {
      import('@tauri-apps/api/core')
        .then(({ invoke }) => invoke('show_main_window'))
        .catch(() => {/* window may already be visible — ignore */});
    } else if (attempt < 30) {
      setTimeout(() => _tryShowWindow(attempt + 1), 100);
    }
  };
  // 50 ms head-start lets WebView2 paint its first React frame on Windows
  // before the window becomes visible, eliminating the DWM white-flash.
  setTimeout(() => _tryShowWindow(0), 50);
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
      {/* DevTools panel only included in development builds */}
      {import.meta.env.DEV && <ReactQueryDevtools initialIsOpen={false} />}
    </QueryClientProvider>
  </StrictMode>,
)
