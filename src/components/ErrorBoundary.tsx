import { Component, ReactNode } from 'react';

interface Props { children: ReactNode; }
interface State { error: Error | null; }

/**
 * Catches any unhandled JS error in its subtree and shows a recoverable
 * error screen instead of a permanent blank white page.
 * Wrap the root route tree with this so a single page crash never kills
 * the whole app.
 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="min-h-screen flex flex-col items-center justify-center bg-slate-900 p-8">
        <div className="bg-slate-800 border border-red-700 rounded-xl p-8 max-w-lg w-full text-center shadow-xl">
          <div className="text-red-400 text-4xl mb-4">⚠</div>
          <h2 className="text-white text-lg font-bold mb-2">Something went wrong</h2>
          <p className="text-slate-400 text-sm mb-6">
            An unexpected error occurred on this page. Your data is safe — click below to reload.
          </p>
          <p className="text-slate-500 text-xs font-mono bg-slate-900 rounded p-3 mb-6 text-left break-all">
            {error.message}
          </p>
          <button
            onClick={() => { this.setState({ error: null }); window.location.reload(); }}
            className="px-6 py-2 bg-blue-600 hover:bg-blue-700 text-white text-sm font-semibold rounded-lg transition-colors"
          >
            Reload App
          </button>
        </div>
      </div>
    );
  }
}
