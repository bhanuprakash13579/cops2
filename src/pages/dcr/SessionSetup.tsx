import { useState } from 'react';
import { Calendar, Sun, Moon, Users, ArrowRight } from 'lucide-react';
import api from '@/lib/api';
import type { DrSession } from './revenueCalc';

interface Props {
  onSessionReady: (session: DrSession) => void;
}

export default function SessionSetup({ onSessionReady }: Props) {
  const today = new Date().toISOString().slice(0, 10);
  const [date, setDate] = useState(today);
  const [shift, setShift] = useState<'DAY' | 'NIGHT'>('DAY');
  const [batchName, setBatchName] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!date || !batchName.trim()) { setError('All fields are required.'); return; }
    setLoading(true);
    setError('');
    try {
      const res = await api.post('/dcr/sessions', {
        report_date: date,
        shift,
        batch_name: batchName.trim(),
      });
      onSessionReady(res.data as DrSession);
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail;
      setError(msg || 'Failed to create session. Please try again.');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-900 p-4">
      <div className="bg-white rounded-2xl shadow-2xl w-full max-w-md p-8">
        <div className="text-center mb-8">
          <div className="inline-flex items-center justify-center w-16 h-16 rounded-2xl bg-teal-100 mb-4">
            <Calendar className="w-8 h-8 text-teal-600" />
          </div>
          <h1 className="text-2xl font-bold text-slate-800">Revenue Report</h1>
          <p className="text-slate-500 text-sm mt-1">Start a new duty collection session</p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-5">
          {/* Date */}
          <div>
            <label className="block text-xs font-semibold text-slate-600 mb-1.5 uppercase tracking-wider">
              Report Date
            </label>
            <input
              type="date"
              value={date}
              onChange={e => setDate(e.target.value)}
              className="w-full border border-slate-300 rounded-xl px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-teal-400"
              required
            />
          </div>

          {/* Shift */}
          <div>
            <label className="block text-xs font-semibold text-slate-600 mb-1.5 uppercase tracking-wider">
              Shift
            </label>
            <div className="grid grid-cols-2 gap-3">
              <button
                type="button"
                onClick={() => setShift('DAY')}
                className={`flex items-center justify-center gap-2 py-3 rounded-xl border-2 font-semibold text-sm transition-all ${
                  shift === 'DAY'
                    ? 'border-amber-400 bg-amber-50 text-amber-700'
                    : 'border-slate-200 text-slate-500 hover:border-slate-300'
                }`}
              >
                <Sun size={16} />
                Day Shift
              </button>
              <button
                type="button"
                onClick={() => setShift('NIGHT')}
                className={`flex items-center justify-center gap-2 py-3 rounded-xl border-2 font-semibold text-sm transition-all ${
                  shift === 'NIGHT'
                    ? 'border-indigo-400 bg-indigo-50 text-indigo-700'
                    : 'border-slate-200 text-slate-500 hover:border-slate-300'
                }`}
              >
                <Moon size={16} />
                Night Shift
              </button>
            </div>
          </div>

          {/* Batch Name */}
          <div>
            <label className="block text-xs font-semibold text-slate-600 mb-1.5 uppercase tracking-wider">
              Batch Name
            </label>
            <div className="relative">
              <Users size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400" />
              <input
                type="text"
                value={batchName}
                onChange={e => setBatchName(e.target.value)}
                placeholder="e.g. C Batch"
                className="w-full border border-slate-300 rounded-xl pl-10 pr-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-teal-400"
                required
              />
            </div>
          </div>

          {error && (
            <p className="text-sm text-red-600 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
              {error}
            </p>
          )}

          <button
            type="submit"
            disabled={loading}
            className="w-full flex items-center justify-center gap-2 bg-teal-600 hover:bg-teal-700 text-white font-semibold py-3 rounded-xl transition-colors disabled:opacity-60"
          >
            {loading ? 'Starting…' : <>Open Sheet <ArrowRight size={16} /></>}
          </button>
        </form>
      </div>
    </div>
  );
}
