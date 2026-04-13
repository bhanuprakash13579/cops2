import { AlertTriangle } from 'lucide-react';
import { useAppMode } from '@/hooks/useAppMode';

export default function DevModeBanner() {
  const { isProd, isLoading } = useAppMode();

  if (isLoading || isProd) return null;

  return (
    <div
      className="fixed top-0 left-0 right-0 z-[9999] flex items-center justify-center gap-2 bg-amber-400 text-amber-900 text-xs font-semibold py-1 px-4 print:hidden"
      style={{ height: '28px' }}
    >
      <AlertTriangle size={13} />
      <span>DEVELOPMENT MODE — Security restrictions are relaxed. Do not use with real data.</span>
    </div>
  );
}
