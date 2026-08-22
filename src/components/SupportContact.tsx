import { open as openUrl } from '@tauri-apps/plugin-shell';
import { Mail, Globe } from 'lucide-react';

/**
 * A small, safe "if this keeps happening, reach us" line, shown when something
 * goes wrong so an officer is never left staring at an error with nowhere to
 * turn. It carries only the public support address and website — never any
 * detail about the machine or the data — so it is safe to show in production.
 *
 * The technical cause of an error is a SEPARATE concern: show that only in
 * development (see the callers' `isProd` gate), never to a production user, where
 * it would help nobody and could tell a probing attacker how the system is built.
 */
export default function SupportContact({ className = '' }: { className?: string }) {
  return (
    <span className={`inline-flex flex-wrap items-center gap-x-3 gap-y-0.5 ${className}`}>
      <span>If this keeps happening, contact us:</span>
      <button
        type="button"
        onClick={() => openUrl('mailto:contact@gsicorp.in').catch(() => {})}
        className="inline-flex items-center gap-1 underline hover:opacity-80"
      >
        <Mail size={11} /> contact@gsicorp.in
      </button>
      <button
        type="button"
        onClick={() => openUrl('https://www.gsicorp.in').catch(() => {})}
        className="inline-flex items-center gap-1 underline hover:opacity-80"
      >
        <Globe size={11} /> www.gsicorp.in
      </button>
    </span>
  );
}
