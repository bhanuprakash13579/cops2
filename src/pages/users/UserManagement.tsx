import React, { useState, useEffect } from 'react';
import { useAuth } from '@/contexts/AuthContext';
import { Users, UserPlus, ShieldAlert, CheckCircle, XCircle } from 'lucide-react';
import api from '@/lib/api';

interface User {
  user_id: string;
  user_name: string;
  user_desig: string;
  user_role: string;
  user_status: string;
}

export default function UserManagement({ moduleType }: { moduleType: 'sdo' | 'adjudication' }) {
  const { token, user: currentUser, logout } = useAuth();
  const [users, setUsers] = useState<User[]>([]);
  const [newUser, setNewUser] = useState({ user_id: '', user_name: '', user_pwd: '', user_desig: '', user_role: moduleType === 'sdo' ? 'SDO' : 'AC' });
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState({ type: '', text: '' });

  const availableRoles = moduleType === 'sdo' ? ['SDO'] : ['AC', 'DC'];

  const fetchUsers = async () => {
    try {
      const res = await api.get('/auth/users', { headers: { Authorization: `Bearer ${token}` } });
      const filtered = res.data.filter((u: User) => availableRoles.includes(u.user_role) && u.user_status !== 'TEMP');
      setUsers(filtered);
    } catch (err) {
      import.meta.env.DEV && console.error(err);
    }
  };

  useEffect(() => {
    if (token) fetchUsers();
  }, [token, moduleType]);

  const handleCreateUser = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setMessage({ type: '', text: '' });
    
    // Safety fallback: if state is stale due to hot-reloads, explicitly enforce the role
    const finalRole = moduleType === 'sdo' ? 'SDO' : newUser.user_role;
    
    try {
      await api.post('/auth/users', { user_name: newUser.user_name, user_desig: newUser.user_desig, user_id: newUser.user_id, password: newUser.user_pwd, user_role: finalRole, user_status: 'ACTIVE' }, { headers: { Authorization: `Bearer ${token}` } });
      setMessage({ type: 'success', text: `User ${newUser.user_id} created successfully!` });
      setNewUser({ user_id: '', user_name: '', user_pwd: '', user_desig: '', user_role: moduleType === 'sdo' ? 'SDO' : 'AC' });
      fetchUsers();
    } catch (err: any) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to create user' });
    } finally {
      setLoading(false);
    }
  };

  const handleCloseUser = async (userId: string) => {
    if (!window.confirm(`Are you sure you want to CLOSE your account? This action cannot be undone.`)) return;
    try {
      await api.delete(`/auth/users/${userId}`, { headers: { Authorization: `Bearer ${token}` } });
      setMessage({ type: 'success', text: 'Account closed. Logging out…' });
      if (userId === currentUser?.user_id) {
        setTimeout(() => { logout(); window.location.href = '/modules'; }, 1200);
      }
    } catch (err: any) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to close user' });
    }
  };

  const handleUpgradeRole = async (userId: string) => {
    if (!window.confirm(`Upgrade user ${userId} from AC to DC?`)) return;
    try {
      await api.patch(`/auth/users/${userId}/role`, { user_role: 'DC' }, { headers: { Authorization: `Bearer ${token}` } });
      setMessage({ type: 'success', text: `User ${userId} upgraded to DC.` });
      fetchUsers();
    } catch (err: any) {
      setMessage({ type: 'error', text: err.response?.data?.detail || 'Failed to upgrade role' });
    }
  };

  return (
    <div className="max-w-6xl mx-auto space-y-6">
      <div className="bg-white p-6 rounded-xl shadow-sm border border-slate-200">
        <div className="flex items-center gap-3 mb-6">
          <div className={`p-3 rounded-lg ${moduleType === 'sdo' ? 'bg-blue-100 text-blue-600' : 'bg-amber-100 text-amber-600'}`}>
            <Users size={24} />
          </div>
          <div>
            <h1 className="text-2xl font-bold text-slate-800 tracking-tight">User Administration</h1>
            <p className="text-slate-500 text-sm">Manage {moduleType.toUpperCase()} module access and roles.</p>
          </div>
        </div>

        {currentUser?.user_status === 'TEMP' && (
          <div className="bg-red-50 border border-red-200 p-4 rounded-lg mb-6 flex items-start gap-3 text-red-700">
            <ShieldAlert className="shrink-0 mt-0.5" />
            <div>
              <h3 className="font-bold">Temporary Session Active</h3>
              <p className="text-sm">You are logged in using one-time bootstrap credentials. You MUST create at least one permanent Admin user below. Once created, this temporary session will self-destruct.</p>
            </div>
          </div>
        )}

        {message.text && (
          <div className={`p-4 rounded-lg mb-6 flex items-center gap-2 ${message.type === 'success' ? 'bg-green-50 text-green-700 border border-green-200' : 'bg-red-50 text-red-700 border border-red-200'}`}>
            {message.type === 'success' ? <CheckCircle size={18} /> : <XCircle size={18} />}
            <span className="font-medium">{message.text}</span>
          </div>
        )}

        {/* Create User Form */}
        <div className="bg-slate-50 p-5 rounded-lg border border-slate-200 mb-8">
          <h2 className="text-sm font-bold text-slate-700 uppercase tracking-wider mb-4 flex items-center gap-2">
            <UserPlus size={16} /> Create New User
          </h2>
          <form onSubmit={handleCreateUser} className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">User ID (Email/Login ID) *</label>
              <input required type="text" value={newUser.user_id} onChange={e => setNewUser({...newUser, user_id: e.target.value})} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Full Name *</label>
              <input required type="text" value={newUser.user_name} onChange={e => setNewUser({...newUser, user_name: e.target.value})} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Password *</label>
              <input required type="password" value={newUser.user_pwd} onChange={e => setNewUser({...newUser, user_pwd: e.target.value})} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Role *</label>
              <select value={newUser.user_role} onChange={e => setNewUser({...newUser, user_role: e.target.value})} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                {availableRoles.map(r => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div className="flex items-end">
              <button disabled={loading} type="submit" className={`w-full py-2 px-4 text-white font-bold rounded shadow-sm transition-colors ${loading ? 'bg-slate-400' : moduleType === 'sdo' ? 'bg-blue-600 hover:bg-blue-700' : 'bg-amber-600 hover:bg-amber-700'}`}>
                {loading ? 'Adding...' : 'Add User'}
              </button>
            </div>
          </form>
        </div>

        {/* Existing Users Table */}
        <h2 className="text-sm font-bold text-slate-700 uppercase tracking-wider mb-4">Active Module Users</h2>
        <div className="overflow-auto border border-slate-200 rounded-lg">
          <table className="w-full text-left text-sm text-slate-600">
            <thead className="bg-slate-100 text-slate-700 uppercase font-semibold text-xs">
              <tr>
                <th className="p-3">User ID</th>
                <th className="p-3">Name</th>
                <th className="p-3">Role</th>
                <th className="p-3">Status</th>
                <th className="p-3 text-right">Action</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {users.map(u => (
                <tr key={u.user_id} className="hover:bg-slate-50 transition-colors">
                  <td className="p-3 font-medium text-slate-900">{u.user_id}</td>
                  <td className="p-3">{u.user_name}</td>
                  <td className="p-3">
                    <span className={`px-2 py-1 rounded-full text-xs font-bold ${u.user_role.includes('Admin') ? 'bg-purple-100 text-purple-700' : 'bg-slate-200 text-slate-700'}`}>
                      {u.user_role}
                    </span>
                  </td>
                  <td className="p-3">
                    <span className={`px-2 py-1 rounded-full text-xs font-bold ${u.user_status === 'ACTIVE' ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}`}>
                      {u.user_status}
                    </span>
                  </td>
                  <td className="p-3 text-right">
                    <div className="flex items-center justify-end gap-3">
                      {moduleType === 'adjudication' && u.user_role === 'AC' && (
                        <button onClick={() => handleUpgradeRole(u.user_id)} className="text-amber-600 hover:text-amber-800 font-medium text-xs underline">
                          Upgrade to DC
                        </button>
                      )}
                      {u.user_status === 'ACTIVE' && u.user_id === currentUser?.user_id && (
                        <button onClick={() => handleCloseUser(u.user_id)} className="text-red-600 hover:text-red-800 font-medium text-xs underline px-2 py-1 border border-red-200 rounded hover:bg-red-50">
                          Close My Account
                        </button>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
              {users.length === 0 && (
                <tr>
                  <td colSpan={5} className="p-8 text-center text-slate-500 italic">No permanent users found. Please create one above.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
