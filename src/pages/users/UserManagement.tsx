import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '@/contexts/AuthContext';
import {
  Users, UserPlus, KeyRound, ArrowRightLeft, Ban, ArrowUpCircle,
  ShieldCheck, CheckCircle, XCircle,
} from 'lucide-react';
import api from '@/lib/api';

interface User {
  id: number;
  user_id: string;
  user_name: string;
  user_desig?: string;
  user_role: string;
  user_status: string;
  is_user_admin?: boolean;
}

/**
 * The user admin's screen — the one AC/DC the system admin designated to keep
 * the office's accounts. Only they can reach this (gated at the route and in the
 * menu, and every action is re-checked on the server). They add officers of any
 * role, close accounts, reset a forgotten password to a one-time value the owner
 * must replace, and — when it is time — hand the whole responsibility to another
 * AC or DC.
 *
 * No password is ever shown here. Passwords are stored only as one-way hashes;
 * the admin sets the temporary value and nothing more.
 */
export default function UserManagement() {
  const { user: currentUser, logout } = useAuth();
  const navigate = useNavigate();
  const [users, setUsers] = useState<User[]>([]);
  const [newUser, setNewUser] = useState({
    user_id: '', user_name: '', user_pwd: '', user_desig: '', user_role: 'SDO',
  });
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState({ type: '', text: '' });

  const say = (type: 'success' | 'error', text: string) => setMessage({ type, text });

  const fetchUsers = async () => {
    try {
      const res = await api.get('/auth/users');
      setUsers(res.data as User[]);
    } catch (err) {
      import.meta.env.DEV && console.error(err);
    }
  };

  useEffect(() => { fetchUsers(); }, []);

  const handleCreateUser = async (e: React.FormEvent) => {
    e.preventDefault();
    if (newUser.user_pwd.length < 6) {
      say('error', 'The password must be at least 6 characters.');
      return;
    }
    setLoading(true);
    setMessage({ type: '', text: '' });
    try {
      await api.post('/auth/users', {
        user_name: newUser.user_name, user_desig: newUser.user_desig,
        user_id: newUser.user_id, password: newUser.user_pwd, user_role: newUser.user_role,
      });
      say('success', `User ${newUser.user_id} created.`);
      setNewUser({ user_id: '', user_name: '', user_pwd: '', user_desig: '', user_role: 'SDO' });
      fetchUsers();
    } catch (err: any) {
      say('error', err.response?.data?.detail || 'Failed to create user.');
    } finally {
      setLoading(false);
    }
  };

  const handleResetPassword = async (u: User) => {
    const temp = window.prompt(
      `Set a temporary password for ${u.user_name} (${u.user_id}).\n\n` +
      `They will sign in with it once and must then choose their own. ` +
      `You will not see their new password.`,
    );
    if (temp === null) return;                 // cancelled
    if (temp.length < 6) { say('error', 'The temporary password must be at least 6 characters.'); return; }
    try {
      await api.post(`/auth/users/${u.id}/reset-password`, { temp_password: temp });
      say('success', `Temporary password set for ${u.user_id}. They must change it at next sign-in.`);
      fetchUsers();
    } catch (err: any) {
      say('error', err.response?.data?.detail || 'Failed to reset the password.');
    }
  };

  const handleClose = async (u: User) => {
    const self = u.user_id === currentUser?.user_id;
    const prompt = self
      ? 'Close YOUR OWN account? You will be signed out.'
      : `Close the account of ${u.user_name} (${u.user_id})? They will no longer be able to sign in.`;
    if (!window.confirm(prompt)) return;
    try {
      await api.delete(`/auth/users/${u.id}`);
      say('success', self ? 'Your account was closed. Signing out…' : `${u.user_id}'s account was closed.`);
      if (self) { setTimeout(() => { logout(); window.location.href = '/modules'; }, 1200); return; }
      fetchUsers();
    } catch (err: any) {
      say('error', err.response?.data?.detail || 'Failed to close the account.');
    }
  };

  const handleUpgrade = async (u: User) => {
    if (!window.confirm(`Upgrade ${u.user_id} from AC to DC?`)) return;
    try {
      await api.patch(`/auth/users/${u.user_id}/role`, { user_role: 'DC' });
      say('success', `${u.user_id} upgraded to DC.`);
      fetchUsers();
    } catch (err: any) {
      say('error', err.response?.data?.detail || 'Failed to upgrade the role.');
    }
  };

  const handleTransfer = async (u: User) => {
    if (!window.confirm(
      `Hand over the user-admin role to ${u.user_name} (${u.user_id})?\n\n` +
      `You will lose the ability to manage accounts, and will be signed out so ` +
      `the change takes effect.`,
    )) return;
    try {
      await api.post('/auth/user-admin/transfer', { user_id: u.user_id });
      say('success', `${u.user_id} is now the user admin. Signing you out…`);
      setTimeout(() => { logout(); window.location.href = '/modules'; }, 1500);
    } catch (err: any) {
      say('error', err.response?.data?.detail || 'Failed to hand over the role.');
    }
  };

  return (
    <div className="max-w-6xl mx-auto space-y-6">
      <div className="bg-white p-6 rounded-xl shadow-sm border border-slate-200">
        <div className="flex items-center gap-3 mb-6">
          <div className="p-3 rounded-lg bg-amber-100 text-amber-600">
            <Users size={24} />
          </div>
          <div>
            <h1 className="text-2xl font-bold text-slate-800 tracking-tight">User Administration</h1>
            <p className="text-slate-500 text-sm">
              Add officers, close accounts, reset forgotten passwords, and hand over this role.
            </p>
          </div>
        </div>

        {message.text && (
          <div className={`p-4 rounded-lg mb-6 flex items-center gap-2 ${message.type === 'success' ? 'bg-green-50 text-green-700 border border-green-200' : 'bg-red-50 text-red-700 border border-red-200'}`}>
            {message.type === 'success' ? <CheckCircle size={18} /> : <XCircle size={18} />}
            <span className="font-medium">{message.text}</span>
          </div>
        )}

        {/* Create User */}
        <div className="bg-slate-50 p-5 rounded-lg border border-slate-200 mb-8">
          <h2 className="text-sm font-bold text-slate-700 uppercase tracking-wider mb-4 flex items-center gap-2">
            <UserPlus size={16} /> Create New User
          </h2>
          <form onSubmit={handleCreateUser} className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Login ID *</label>
              <input required type="text" value={newUser.user_id} onChange={e => setNewUser({ ...newUser, user_id: e.target.value })} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Full Name *</label>
              <input required type="text" value={newUser.user_name} onChange={e => setNewUser({ ...newUser, user_name: e.target.value })} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Password *</label>
              <input required type="password" value={newUser.user_pwd} onChange={e => setNewUser({ ...newUser, user_pwd: e.target.value })} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 outline-none" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Role *</label>
              <select value={newUser.user_role} onChange={e => setNewUser({ ...newUser, user_role: e.target.value })} className="w-full text-sm p-2 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 outline-none bg-white">
                <option value="SDO">SDO</option>
                <option value="AC">AC</option>
                <option value="DC">DC</option>
              </select>
            </div>
            <div className="flex items-end">
              <button disabled={loading} type="submit" className={`w-full py-2 px-4 text-white font-bold rounded shadow-sm transition-colors ${loading ? 'bg-slate-400' : 'bg-amber-600 hover:bg-amber-700'}`}>
                {loading ? 'Adding…' : 'Add User'}
              </button>
            </div>
          </form>
          <p className="text-xs text-slate-500 mt-3">
            The user admin can create any role. Designating another user admin is done by the
            system administrator, or by handing this role over below.
          </p>
        </div>

        {/* Existing Users */}
        <h2 className="text-sm font-bold text-slate-700 uppercase tracking-wider mb-4">Users</h2>
        <div className="overflow-auto border border-slate-200 rounded-lg">
          <table className="w-full text-left text-sm text-slate-600">
            <thead className="bg-slate-100 text-slate-700 uppercase font-semibold text-xs">
              <tr>
                <th className="p-3">Login ID</th>
                <th className="p-3">Name</th>
                <th className="p-3">Role</th>
                <th className="p-3">Status</th>
                <th className="p-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {users.map(u => {
                const self = u.user_id === currentUser?.user_id;
                return (
                  <tr key={u.user_id} className="hover:bg-slate-50 transition-colors">
                    <td className="p-3 font-medium text-slate-900">{u.user_id}</td>
                    <td className="p-3">{u.user_name}</td>
                    <td className="p-3">
                      <div className="flex items-center gap-2">
                        <span className="px-2 py-1 rounded-full text-xs font-bold bg-slate-200 text-slate-700">{u.user_role}</span>
                        {u.is_user_admin && (
                          <span className="px-2 py-1 rounded-full text-xs font-bold bg-purple-100 text-purple-700 inline-flex items-center gap-1">
                            <ShieldCheck size={12} /> User Admin
                          </span>
                        )}
                      </div>
                    </td>
                    <td className="p-3">
                      <span className={`px-2 py-1 rounded-full text-xs font-bold ${
                        u.user_status === 'ACTIVE' ? 'bg-green-100 text-green-700'
                        : u.user_status === 'TEMP' ? 'bg-amber-100 text-amber-700'
                        : 'bg-red-100 text-red-700'}`}>
                        {u.user_status === 'TEMP' ? 'MUST RESET PASSWORD' : u.user_status}
                      </span>
                    </td>
                    <td className="p-3 text-right">
                      <div className="flex items-center justify-end gap-3 flex-wrap">
                        {!self && (
                          <button onClick={() => handleResetPassword(u)} title="Set a one-time password the user must change"
                            className="text-slate-600 hover:text-slate-900 font-medium text-xs inline-flex items-center gap-1">
                            <KeyRound size={13} /> Reset password
                          </button>
                        )}
                        {u.user_role === 'AC' && (
                          <button onClick={() => handleUpgrade(u)}
                            className="text-amber-600 hover:text-amber-800 font-medium text-xs inline-flex items-center gap-1">
                            <ArrowUpCircle size={13} /> Upgrade to DC
                          </button>
                        )}
                        {!self && !u.is_user_admin && (u.user_role === 'AC' || u.user_role === 'DC') && (
                          <button onClick={() => handleTransfer(u)} title="Hand the user-admin role to this officer"
                            className="text-purple-600 hover:text-purple-800 font-medium text-xs inline-flex items-center gap-1">
                            <ArrowRightLeft size={13} /> Make user admin
                          </button>
                        )}
                        {u.user_status !== 'CLOSED' && (
                          <button onClick={() => handleClose(u)}
                            className="text-red-600 hover:text-red-800 font-medium text-xs inline-flex items-center gap-1 px-2 py-1 border border-red-200 rounded hover:bg-red-50">
                            <Ban size={13} /> {self ? 'Close my account' : 'Close'}
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
              {users.length === 0 && (
                <tr>
                  <td colSpan={5} className="p-8 text-center text-slate-500 italic">No users found.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
      <div className="text-center">
        <button onClick={() => navigate('/adjudication')} className="text-sm text-slate-500 hover:text-slate-700 underline">
          ← Back to Adjudication
        </button>
      </div>
    </div>
  );
}
