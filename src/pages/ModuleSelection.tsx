import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { ClipboardEdit, Gavel, ArrowRight, Search, ScanLine, ExternalLink, Mail } from 'lucide-react';
import { open as openUrl } from '@tauri-apps/plugin-shell';
import api from '@/lib/api';

export default function ModuleSelection() {
  const navigate = useNavigate();
  const [secretClicks, setSecretClicks] = useState(0);
  const [apisEnabled, setApisEnabled] = useState(() => localStorage.getItem('cops_apis_enabled') === 'true');

  useEffect(() => {
    api.get('/features')
      .then(r => {
        const enabled = !!r.data.apis_enabled;
        setApisEnabled(enabled);
        localStorage.setItem('cops_apis_enabled', String(enabled));
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!secretClicks) return;
    const t = setTimeout(() => setSecretClicks(0), 1500);
    return () => clearTimeout(t);
  }, [secretClicks]);

  const handleSecretClick = () => {
    setSecretClicks(c => {
      const next = c + 1;
      if (next >= 4) {
        navigate('/restore-backup');
        return 0;
      }
      return next;
    });
  };

  return (
    <div className="min-h-screen flex flex-col justify-center items-center relative overflow-hidden" style={{ background: 'linear-gradient(135deg, #0f172a 0%, #1e3a5f 50%, #0f172a 100%)' }}>
      {/* Background orbs — blur removed, radial-gradient already looks soft */}
      <div className="absolute top-[-10%] right-[-5%] w-[500px] h-[500px] rounded-full opacity-20" style={{ background: 'radial-gradient(circle, #3b82f6, transparent)' }} />
      <div className="absolute bottom-[-10%] left-[-5%] w-[500px] h-[500px] rounded-full opacity-20" style={{ background: 'radial-gradient(circle, #f59e0b, transparent)' }} />

      <div className={`w-full px-6 z-10 ${apisEnabled ? 'max-w-7xl' : 'max-w-4xl'}`}>
        {/* Header */}
        <div className="text-center mb-12">
          <div className="flex justify-center mb-4">
            <div className="bg-white/10 border border-white/20 p-4 rounded-2xl shadow-2xl">
              <ClipboardEdit className="w-12 h-12 text-blue-400" />
            </div>
          </div>
          <h1 className="text-4xl font-extrabold text-white tracking-tight mb-2">
            COPS
          </h1>
          <p className="text-blue-200 text-lg font-medium">CBIC · Customs · Chennai Airport</p>
        </div>

        {/* Module Cards */}
        <div className={`grid grid-cols-1 md:grid-cols-2 ${apisEnabled ? 'lg:grid-cols-4' : 'lg:grid-cols-3'} gap-5 mb-10`}>

          {/* SDO Module Card */}
          <button
            onClick={() => navigate('/login/sdo')}
            className="group relative bg-white/10 border border-white/20 rounded-2xl p-6 text-left hover:bg-white/20 hover:border-blue-400/50 hover:shadow-2xl hover:shadow-blue-500/20 transition-[transform,box-shadow,background-color,border-color] duration-300 hover:-translate-y-1 focus:outline-none focus:ring-2 focus:ring-blue-400"
            id="btn-sdo-module"
          >
            <div className="absolute inset-x-0 top-0 h-0.5 bg-gradient-to-r from-transparent via-blue-400 to-transparent rounded-t-2xl opacity-0 group-hover:opacity-100 transition-opacity" />

            <div className="flex items-start justify-between mb-6">
              <div className="bg-blue-500/20 border border-blue-400/30 p-4 rounded-xl group-hover:bg-blue-500/30 transition-colors">
                <ClipboardEdit className="w-8 h-8 text-blue-300" />
              </div>
              <ArrowRight className="text-white/30 group-hover:text-blue-300 group-hover:translate-x-1 transition-[transform,color] mt-2" size={24} />
            </div>

            <div className="space-y-2 mb-6">
              <h2 className="text-2xl font-bold text-white">SDO Module</h2>
              <p className="text-blue-200 text-sm font-medium uppercase tracking-widest">Offence Case Registration</p>
            </div>

            <div className="mt-6 pt-6 border-t border-white/10">
              <span className="text-xs text-blue-300/70 uppercase tracking-wider">
                Access: SDO
              </span>
            </div>
          </button>

          {/* Adjudication Module Card */}
          <button
            onClick={() => navigate('/login/adjudication')}
            className="group relative bg-white/10 border border-white/20 rounded-2xl p-6 text-left hover:bg-white/20 hover:border-amber-400/50 hover:shadow-2xl hover:shadow-amber-500/20 transition-[transform,box-shadow,background-color,border-color] duration-300 hover:-translate-y-1 focus:outline-none focus:ring-2 focus:ring-amber-400"
            id="btn-adjudication-module"
          >
            <div className="absolute inset-x-0 top-0 h-0.5 bg-gradient-to-r from-transparent via-amber-400 to-transparent rounded-t-2xl opacity-0 group-hover:opacity-100 transition-opacity" />

            <div className="flex items-start justify-between mb-6">
              <div className="bg-amber-500/20 border border-amber-400/30 p-4 rounded-xl group-hover:bg-amber-500/30 transition-colors">
                <Gavel className="w-8 h-8 text-amber-300" />
              </div>
              <ArrowRight className="text-white/30 group-hover:text-amber-300 group-hover:translate-x-1 transition-[transform,color] mt-2" size={24} />
            </div>

            <div className="space-y-2 mb-6">
              <h2 className="text-2xl font-bold text-white">Adjudication Module</h2>
              <p className="text-amber-200 text-sm font-medium uppercase tracking-widest">Online Adjudication</p>
            </div>

            <div className="mt-6 pt-6 border-t border-white/10">
              <span className="text-xs text-amber-300/70 uppercase tracking-wider">
                Access: DC · AC
              </span>
            </div>
          </button>

          {/* Query & Printing Module Card */}
          <button
            onClick={() => navigate('/login/query')}
            className="group relative bg-white/10 border border-white/20 rounded-2xl p-6 text-left hover:bg-white/20 hover:border-emerald-400/50 hover:shadow-2xl hover:shadow-emerald-500/20 transition-[transform,box-shadow,background-color,border-color] duration-300 hover:-translate-y-1 focus:outline-none focus:ring-2 focus:ring-emerald-400"
            id="btn-query-module"
          >
            <div className="absolute inset-x-0 top-0 h-0.5 bg-gradient-to-r from-transparent via-emerald-400 to-transparent rounded-t-2xl opacity-0 group-hover:opacity-100 transition-opacity" />

            <div className="flex items-start justify-between mb-6">
              <div className="bg-emerald-500/20 border border-emerald-400/30 p-4 rounded-xl group-hover:bg-emerald-500/30 transition-colors">
                <Search className="w-8 h-8 text-emerald-300" />
              </div>
              <ArrowRight className="text-white/30 group-hover:text-emerald-300 group-hover:translate-x-1 transition-[transform,color] mt-2" size={24} />
            </div>

            <div className="space-y-2 mb-6">
              <h2 className="text-2xl font-bold text-white">Query Module</h2>
              <p className="text-emerald-200 text-sm font-medium uppercase tracking-widest">Search & Reports</p>
            </div>

            <div className="mt-6 pt-6 border-t border-white/10">
              <span className="text-xs text-emerald-300/70 uppercase tracking-wider">
                Access: All Authorized Officers
              </span>
            </div>
          </button>

          {/* COPS ↔ APIS Module Card — only shown when enabled by admin */}
          {apisEnabled && <button
            onClick={() => navigate('/login/apis')}
            className="group relative bg-white/10 border border-white/20 rounded-2xl p-6 text-left hover:bg-white/20 hover:border-violet-400/50 hover:shadow-2xl hover:shadow-violet-500/20 transition-[transform,box-shadow,background-color,border-color] duration-300 hover:-translate-y-1 focus:outline-none focus:ring-2 focus:ring-violet-400"
            id="btn-apis-module"
          >
            <div className="absolute inset-x-0 top-0 h-0.5 bg-gradient-to-r from-transparent via-violet-400 to-transparent rounded-t-2xl opacity-0 group-hover:opacity-100 transition-opacity" />

            <div className="flex items-start justify-between mb-6">
              <div className="bg-violet-500/20 border border-violet-400/30 p-4 rounded-xl group-hover:bg-violet-500/30 transition-colors">
                <ScanLine className="w-8 h-8 text-violet-300" />
              </div>
              <ArrowRight className="text-white/30 group-hover:text-violet-300 group-hover:translate-x-1 transition-[transform,color] mt-2" size={24} />
            </div>

            <div className="space-y-2 mb-6">
              <h2 className="text-2xl font-bold text-white">COPS ↔ APIS</h2>
              <p className="text-violet-200 text-sm font-medium uppercase tracking-widest">Passenger Intelligence</p>
            </div>

            <div className="mt-6 pt-6 border-t border-white/10">
              <span className="text-xs text-violet-300/70 uppercase tracking-wider">
                Access: SDO · DC · AC
              </span>
            </div>
          </button>}
        </div>

        {/* Secret backup trigger - invisible, no visual clues */}
        <div
          onClick={handleSecretClick}
          className="absolute bottom-10 left-0 w-16 h-16 z-20"
          style={{ cursor: 'default' }}
        />
      </div>

      {/* Footer */}
      <div className="absolute bottom-0 left-0 right-0 flex flex-col justify-center items-center py-3 gap-1 z-0 pointer-events-none">
        <p className="text-white/30 text-xs tracking-widest uppercase select-none">
          Powered by{' '}
          <span className="text-white/50 font-semibold">Get Some Idea Technologies</span>
        </p>
        <div className="flex items-center gap-4 pointer-events-auto">
          <button
            onClick={() => openUrl('https://www.gsicorp.in').catch(() => {})}
            title="Opens gsicorp.in in your browser"
            aria-label="Visit GSI Corp website (opens in browser)"
            className="flex items-center gap-1 text-white/55 hover:text-blue-300 hover:underline text-xs transition-colors duration-200 focus:outline-none focus:ring-1 focus:ring-blue-400 focus:ring-offset-1 focus:ring-offset-transparent rounded"
          >
            <ExternalLink size={11} />
            www.gsicorp.in
          </button>
          <span className="text-white/20 text-xs select-none">·</span>
          <button
            onClick={() => openUrl('mailto:contact@gsicorp.in').catch(() => {})}
            title="Opens your mail client to contact us"
            aria-label="Send email to contact@gsicorp.in"
            className="flex items-center gap-1 text-white/55 hover:text-blue-300 hover:underline text-xs transition-colors duration-200 focus:outline-none focus:ring-1 focus:ring-blue-400 focus:ring-offset-1 focus:ring-offset-transparent rounded"
          >
            <Mail size={11} />
            contact@gsicorp.in
          </button>
        </div>
      </div>

    </div>
  );
}
