import { useState, useEffect } from 'react';
import { Copy, Check, Settings, MessageSquare } from 'lucide-react';
import api from '@/lib/api';
import type { DrSession } from './revenueCalc';
import { buildMessage } from './revenueCalc';

interface Props {
  session: DrSession;
  onClose: () => void;
}

interface DrSettings {
  id: number;
  greeting_recipients: string;
}

export default function MessageGenerator({ session, onClose }: Props) {
  const [settings, setSettings] = useState<DrSettings>({ id: 1, greeting_recipients: 'Sir' });
  const [editingRecipients, setEditingRecipients] = useState(false);
  const [recipientsInput, setRecipientsInput] = useState(settings.greeting_recipients);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    api.get('/dcr/settings')
      .then(r => {
        setSettings(r.data);
        setRecipientsInput(r.data.greeting_recipients);
      })
      .catch(() => {});
  }, []);

  const message = buildMessage(session, settings);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(message);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* silent */ }
  };

  const saveRecipients = async () => {
    setSaving(true);
    try {
      const res = await api.put('/dcr/settings', { greeting_recipients: recipientsInput.trim() });
      setSettings(res.data);
      setEditingRecipients(false);
    } catch { /* silent */ } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="fixed inset-0 bg-black/40" onClick={onClose} />
      <div className="relative bg-white rounded-2xl shadow-2xl w-full max-w-lg">
        <div className="flex items-center justify-between px-5 py-4 border-b border-slate-200">
          <div className="flex items-center gap-2">
            <MessageSquare size={20} className="text-teal-600" />
            <h2 className="font-bold text-slate-800">WhatsApp Report Message</h2>
          </div>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-600 text-lg font-bold">✕</button>
        </div>

        <div className="p-5 space-y-4">
          {/* Recipients setting */}
          <div className="flex items-center gap-2 text-sm">
            <Settings size={14} className="text-slate-400" />
            <span className="text-slate-500 text-xs">Greeting recipients:</span>
            {editingRecipients ? (
              <div className="flex items-center gap-2 flex-1">
                <input
                  value={recipientsInput}
                  onChange={e => setRecipientsInput(e.target.value)}
                  className="border border-slate-300 rounded-lg px-2 py-1 text-xs flex-1 focus:outline-none focus:ring-2 focus:ring-teal-400"
                  placeholder="e.g. AC Sir/BS Sir"
                />
                <button onClick={saveRecipients} disabled={saving}
                  className="text-xs bg-teal-600 text-white px-3 py-1 rounded-lg disabled:opacity-60">
                  {saving ? '…' : 'Save'}
                </button>
                <button onClick={() => setEditingRecipients(false)}
                  className="text-xs text-slate-500 hover:text-slate-700 px-2 py-1">
                  Cancel
                </button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <span className="font-semibold text-slate-700 text-xs">{settings.greeting_recipients}</span>
                <button onClick={() => setEditingRecipients(true)}
                  className="text-teal-600 hover:underline text-xs">Edit</button>
              </div>
            )}
          </div>

          {/* Message preview */}
          <div className="bg-slate-50 border border-slate-200 rounded-xl p-4 font-mono text-xs leading-relaxed whitespace-pre-wrap text-slate-700 max-h-80 overflow-y-auto">
            {message}
          </div>

          <div className="flex gap-3">
            <button
              onClick={handleCopy}
              className={`flex-1 flex items-center justify-center gap-2 py-2.5 rounded-xl font-semibold text-sm transition-colors ${
                copied
                  ? 'bg-emerald-600 text-white'
                  : 'bg-teal-600 hover:bg-teal-700 text-white'
              }`}
            >
              {copied ? <><Check size={16} /> Copied!</> : <><Copy size={16} /> Copy to Clipboard</>}
            </button>
            <button onClick={onClose}
              className="px-4 py-2.5 bg-slate-100 hover:bg-slate-200 text-slate-700 font-semibold rounded-xl text-sm">
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
