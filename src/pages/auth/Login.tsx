import React, { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { ClipboardEdit, AlertTriangle, KeyRound, User, Loader2, Gavel, Search, ScanLine } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';
import api from '@/lib/api';

export default function Login() {
  const { moduleType } = useParams<{ moduleType: 'sdo' | 'adjudication' | 'query' | 'apis' }>();
  const isSDO = moduleType === 'sdo';
  const isQuery = moduleType === 'query';
  const isAdjn = moduleType === 'adjudication';
  const isApis = moduleType === 'apis';

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [bootstrapInfo, setBootstrapInfo] = useState<{username: string, password: string, message: string} | null>(null);
  
  const { login } = useAuth();
  const navigate = useNavigate();

  React.useEffect(() => {
    const checkBootstrap = async () => {
      try {
        const res = await api.get(`/auth/bootstrap/${moduleType}`);
        if (res.data?.bootstrap_needed && res.data?.credentials) {
          setBootstrapInfo(res.data.credentials);
        }
      } catch (err) {
        import.meta.env.DEV && console.error("Bootstrap check failed", err);
      }
    };
    if (moduleType) checkBootstrap();
  }, [moduleType]);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);

    try {
      const formData = new URLSearchParams();
      formData.append('username', username);
      formData.append('password', password);
      // Pass the requested module to backend to enforce RBAC during token generation
      formData.append('module_type', moduleType || '');

      const response = await api.post('/auth/login', formData, {
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' }
      });

      const { access_token, user } = response.data;
      login(access_token, user);

      // Navigate to the precise module dashboard instead of generic /modules
      if (isSDO) navigate('/sdo');
      else if (isQuery) navigate('/query');
      else if (isApis) navigate('/apis');
      else navigate('/adjudication');

    } catch (err: any) {
      setError(err.response?.data?.detail || 'Invalid credentials or unauthorized for this module.');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={`min-h-screen flex flex-col justify-center py-12 sm:px-6 lg:px-8 relative overflow-hidden ${isSDO ? 'bg-slate-50' : isQuery ? 'bg-emerald-50/50' : isApis ? 'bg-violet-50/50' : 'bg-amber-50/50'}`}>
      
      {/* Decorative background elements */}
      {isSDO && (
        <>
          <div className="absolute top-0 right-[-10%] w-[600px] h-[600px] bg-blue-400/10 rounded-full -z-10"></div>
          <div className="absolute bottom-[-10%] left-[-10%] w-[600px] h-[600px] bg-slate-500/10 rounded-full -z-10"></div>
        </>
      )}
      {isAdjn && (
        <>
          <div className="absolute top-0 right-[-10%] w-[600px] h-[600px] bg-amber-400/20 rounded-full -z-10"></div>
          <div className="absolute bottom-[-10%] left-[-10%] w-[600px] h-[600px] bg-orange-500/10 rounded-full -z-10"></div>
        </>
      )}
      {isQuery && (
        <>
          <div className="absolute top-0 right-[-10%] w-[600px] h-[600px] bg-emerald-400/20 rounded-full -z-10"></div>
          <div className="absolute bottom-[-10%] left-[-10%] w-[600px] h-[600px] bg-teal-500/10 rounded-full -z-10"></div>
        </>
      )}
      {isApis && (
        <>
          <div className="absolute top-0 right-[-10%] w-[600px] h-[600px] bg-violet-400/20 rounded-full -z-10"></div>
          <div className="absolute bottom-[-10%] left-[-10%] w-[600px] h-[600px] bg-purple-500/10 rounded-full -z-10"></div>
        </>
      )}

      <div className="sm:mx-auto sm:w-full sm:max-w-md cursor-pointer" onClick={() => navigate('/modules')}>
        <div className="flex justify-center flex-col items-center group">
          <div className={`p-4 rounded-2xl shadow-lg border transition-transform group-hover:scale-105 ${isSDO ? 'bg-blue-900 border-blue-800' : isQuery ? 'bg-emerald-900 border-emerald-800' : isApis ? 'bg-violet-900 border-violet-800' : 'bg-amber-800 border-amber-700'}`}>
            {isSDO && <ClipboardEdit className="w-12 h-12 text-blue-300" />}
            {isAdjn && <Gavel className="w-12 h-12 text-amber-300" />}
            {isQuery && <Search className="w-12 h-12 text-emerald-300" />}
            {isApis && <ScanLine className="w-12 h-12 text-violet-300" />}
          </div>
          <h2 className={`mt-6 text-center text-3xl font-extrabold tracking-tight ${isSDO ? 'text-blue-900' : isQuery ? 'text-emerald-900' : isApis ? 'text-violet-900' : 'text-amber-900'}`}>
            {isSDO ? 'SDO MODULE' : isQuery ? 'QUERY MODULE' : isApis ? 'COPS ↔ APIS' : 'ADJUDICATION MODULE'}
          </h2>
          <p className="mt-2 text-center text-sm text-slate-500 font-medium">
            COPS
          </p>
        </div>
      </div>

      <div className="mt-8 sm:mx-auto sm:w-full sm:max-w-md relative z-10 mb-10">
        <div className={`py-8 px-4 shadow-2xl sm:rounded-xl sm:px-10 border ${isSDO ? 'bg-white/80 border-blue-100' : isQuery ? 'bg-white/80 border-emerald-100' : isApis ? 'bg-white/80 border-violet-100' : 'bg-white/80 border-amber-200'}`}>
          
          {bootstrapInfo && (
            <div className={`mb-6 p-4 rounded-lg border-2 shadow-lg ${isSDO ? 'bg-blue-50 border-blue-400' : 'bg-amber-50 border-amber-400'}`}>
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 font-bold text-slate-800">
                  <AlertTriangle className={`w-5 h-5 ${isSDO ? 'text-blue-600' : 'text-amber-600'}`} />
                  <h2>First-Time Setup Required</h2>
                </div>
                <p className="text-sm text-slate-600 font-medium">
                  {bootstrapInfo.message}
                </p>
                <div className="mt-3 bg-white p-3 rounded border border-slate-200 shadow-inner font-mono text-sm space-y-1">
                  <div className="flex justify-between">
                    <span className="text-slate-500">Username:</span>
                    <strong className="text-slate-900 select-all">{bootstrapInfo.username}</strong>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-slate-500">Password:</span>
                    <strong className="text-slate-900 select-all">{bootstrapInfo.password}</strong>
                  </div>
                </div>
              </div>
            </div>
          )}

          <form className="space-y-6" onSubmit={handleLogin}>
            
            {error && (
              <div className="bg-red-50 border-l-4 border-red-500 p-4 rounded-md">
                <div className="flex">
                  <div className="ml-3">
                    <p className="text-sm text-red-700 font-medium">{error}</p>
                  </div>
                </div>
              </div>
            )}

            <div>
              <label className="block text-sm font-semibold text-slate-700 mb-2">Login ID / Username</label>
              <div className="relative">
                <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                  <User className="h-5 w-5 text-slate-400" />
                </div>
                <input
                  type="text"
                  required
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className={`w-full pl-10 pr-3 py-2 border rounded-lg focus:outline-none focus:ring-2 transition-colors ${
                    isSDO
                      ? 'border-slate-300 focus:border-blue-500 focus:ring-blue-500/20'
                      : isQuery
                      ? 'border-slate-300 focus:border-emerald-500 focus:ring-emerald-500/20'
                      : isApis
                      ? 'border-slate-300 focus:border-violet-500 focus:ring-violet-500/20'
                      : 'border-slate-300 focus:border-amber-500 focus:ring-amber-500/20'
                  }`}
                  placeholder="Enter your Login ID"
                />
              </div>
            </div>

            <div>
              <label className="block text-sm font-semibold text-slate-700 mb-2">Password</label>
              <div className="relative">
                <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                  <KeyRound className="h-5 w-5 text-slate-400" />
                </div>
                <input
                  type="password"
                  required
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className={`w-full pl-10 pr-3 py-2 border rounded-lg focus:outline-none focus:ring-2 transition-colors ${
                    isSDO
                      ? 'border-slate-300 focus:border-blue-500 focus:ring-blue-500/20'
                      : isQuery
                      ? 'border-slate-300 focus:border-emerald-500 focus:ring-emerald-500/20'
                      : isApis
                      ? 'border-slate-300 focus:border-violet-500 focus:ring-violet-500/20'
                      : 'border-slate-300 focus:border-amber-500 focus:ring-amber-500/20'
                  }`}
                  placeholder="Enter your password"
                />
              </div>
            </div>

            <div className="pt-2">
              <button
                type="submit"
                disabled={loading}
                className={`w-full flex justify-center py-2.5 px-4 border border-transparent rounded-lg shadow-sm text-sm font-bold text-white focus:outline-none focus:ring-2 focus:ring-offset-2 disabled:opacity-70 transition-colors ${
                  isSDO
                    ? 'bg-blue-600 hover:bg-blue-700 focus:ring-blue-500'
                    : isQuery
                    ? 'bg-emerald-600 hover:bg-emerald-700 focus:ring-emerald-600'
                    : isApis
                    ? 'bg-violet-600 hover:bg-violet-700 focus:ring-violet-600'
                    : 'bg-amber-700 hover:bg-amber-800 focus:ring-amber-700'
                }`}
              >
                {loading ? <Loader2 className="w-5 h-5 animate-spin" /> : `Log In to ${isSDO ? 'SDO' : isQuery ? 'Query' : isApis ? 'COPS ↔ APIS' : 'Adjudication'}`}
              </button>
            </div>
            
            <div className="text-center pt-2">
              <button 
                type="button" 
                onClick={() => navigate('/modules')}
                className="text-sm font-medium text-slate-500 hover:text-slate-700 underline underline-offset-2 hover:bg-slate-50 px-3 py-1 rounded"
              >
                ← Back to Module Selection
              </button>
            </div>
          </form>
          

        </div>
      </div>

      {/* Footer */}
      <div className="absolute bottom-0 left-0 right-0 flex justify-center items-center py-3">
        <p className={`text-xs tracking-widest uppercase select-none ${isSDO ? 'text-slate-400/60' : isQuery ? 'text-emerald-900/30' : isApis ? 'text-violet-900/30' : 'text-amber-900/30'}`}>
          Powered by{' '}
          <span className={`font-semibold ${isSDO ? 'text-slate-500/70' : isQuery ? 'text-emerald-900/50' : isApis ? 'text-violet-900/50' : 'text-amber-900/50'}`}>
            Get Some Idea Technologies
          </span>
        </p>
      </div>

    </div>
  );
}
