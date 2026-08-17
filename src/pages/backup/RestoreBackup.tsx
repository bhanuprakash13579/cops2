import { useState, useEffect, useRef, Fragment, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ArrowLeft, ShieldCheck, ShieldAlert, Lock, LogIn,
  UserPlus, Users, Pencil, X, KeyRound, Download,
  Upload, FileUp, Eye, EyeOff, RefreshCw, Database, ToggleLeft, ToggleRight, ScanLine,
  Wifi, Plus, AlertTriangle, Monitor, Settings, Scale, Trash2, Clock, CheckCircle, CalendarDays, CloudUpload, FileDown } from 'lucide-react';
import api from '@/lib/api';
import { showDownloadToast } from '@/components/DownloadToast';
import StatutesAdmin from './StatutesAdmin';
import OSTemplateEditor from './OSTemplateEditor';

// ── Types ─────────────────────────────────────────────────────────────────────

interface AutoBackupDestination {
  path: string;
  reachable: boolean;
  detail: string;
  off_machine: boolean;
}

interface AutoBackupStatus {
  destinations: AutoBackupDestination[];
  any_off_machine: boolean;
  interval_minutes: number;
  keep_copies: number;
  last_success: string | null;
  hours_since_success: number | null;
  healthy: boolean;
  refusing: string | null;
  composition: { table: string; rows: number }[];
  excluded_tables: string | null;
}

interface DeviceInfo {
  mac: string;
  hostname: string;
  registered: boolean;
  key_path: string;
}

interface AllowedDeviceRow {
  id: number;
  label: string;
  ip_address: string | null;
  mac_address: string | null;
  hostname: string | null;
  is_active: boolean;
  added_on: string | null;
  notes: string | null;
}

interface UserRow {
  id: number;
  user_id: string;
  user_name: string;
  user_desig: string;
  user_role: string;
  user_status: string;
  created_on: string | null;
}

const ALL_ROLES = ['SDO', 'DC', 'AC'];

// ── Admin API helper ──────────────────────────────────────────────────────────

function adminHeaders(token: string) {
  return { Authorization: `Bearer ${token}` };
}

// ── Main component ────────────────────────────────────────────────────────────

export default function RestoreBackup() {
  const navigate = useNavigate();

  // Admin session (memory only — never persisted)
  const [adminToken, setAdminToken] = useState('');

  // ── Trial / licence ────────────────────────────────────────────────────────
  // Mirrors the Python app: the administrator owns the expiry date outright —
  // set the length, restart the window, or disable it to make the installation
  // permanent. Nothing here is automatic.
  interface TrialInfo { trial_disabled: boolean; days_remaining: number | null; trial_days: number; expired: boolean }
  const [trialStatus, setTrialStatus]   = useState<TrialInfo | null>(null);
  const [trialDaysInput, setTrialDaysInput] = useState('30');
  const [trialLoading, setTrialLoading] = useState(false);
  const [trialMsg, setTrialMsg]         = useState('');

  const reloadTrial = async () => {
    const r = await api.get('/trial-status');
    const td = r.data.trial_days ?? 30;
    setTrialStatus({
      trial_disabled: !!r.data.trial_disabled,
      days_remaining: r.data.days_remaining ?? null,
      trial_days: td,
      expired: !!r.data.expired,
    });
    setTrialDaysInput(String(td));
    return r.data;
  };

  const handleTrialReset = async () => {
    setTrialLoading(true); setTrialMsg('');
    try {
      const res = await api.post('/admin/trial/reset', {}, { headers: adminHeaders(adminToken) });
      await reloadTrial();
      setTrialMsg(`Trial reset - ${res.data?.trial_days ?? 30}-day window starts today.`);
    } catch (err: any) {
      setTrialMsg(err.response?.data?.detail || 'Failed to reset trial.');
    } finally { setTrialLoading(false); }
  };

  const handleTrialDisable = async () => {
    setTrialLoading(true); setTrialMsg('');
    try {
      await api.post('/admin/trial/disable', {}, { headers: adminHeaders(adminToken) });
      setTrialStatus(prev => ({
        trial_disabled: true,
        days_remaining: null,
        trial_days: prev?.trial_days ?? 30,
        expired: false,
      }));
      setTrialMsg('Trial disabled - installation is now permanent.');
    } catch (err: any) {
      setTrialMsg(err.response?.data?.detail || 'Failed to disable trial.');
    } finally { setTrialLoading(false); }
  };

  const handleTrialDaysSave = async () => {
    const days = parseInt(trialDaysInput, 10);
    if (isNaN(days) || days < 1 || days > 3650) {
      setTrialMsg('Enter a number of days between 1 and 3650.');
      return;
    }
    setTrialLoading(true); setTrialMsg('');
    try {
      await api.post('/admin/trial/set-days', { trial_days: days }, { headers: adminHeaders(adminToken) });
      await reloadTrial();
      setTrialMsg(`Trial set to ${days} day${days === 1 ? '' : 's'}.`);
    } catch (err: any) {
      setTrialMsg(err.response?.data?.detail || 'Failed to update trial duration.');
    } finally { setTrialLoading(false); }
  };

  const [loginUser, setLoginUser] = useState('');
  const [loginPwd, setLoginPwd] = useState('');
  const [showLoginPwd, setShowLoginPwd] = useState(false);
  const [loginErr, setLoginErr] = useState('');
  const [loginLoading, setLoginLoading] = useState(false);

  // Device
  const [deviceInfo, setDeviceInfo] = useState<DeviceInfo | null>(null);
  const [regLoading, setRegLoading] = useState(false);
  const [regMsg, setRegMsg] = useState('');

  // Users
  const [users, setUsers] = useState<UserRow[]>([]);
  const [usersLoading, setUsersLoading] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [newUser, setNewUser] = useState({ user_id: '', user_name: '', user_desig: '', user_pwd: '', user_role: 'SDO' });
  const [showNewPwd, setShowNewPwd] = useState(false);
  const [createErr, setCreateErr] = useState('');
  const [createMsg, setCreateMsg] = useState('');

  // Edit user
  const [editId, setEditId] = useState<number | null>(null);
  const [editData, setEditData] = useState({ user_name: '', user_desig: '', user_pwd: '', user_role: '', user_status: '' });
  const [showEditPwd, setShowEditPwd] = useState(false);
  const [editMsg, setEditMsg] = useState('');

  // Feature flags
  const [apisEnabled, setApisEnabled] = useState(false);
  const [flagsLoading, setFlagsLoading] = useState(false);
  const [flagsMsg, setFlagsMsg] = useState('');

  // The monthly copy that leaves the building. The token is typed here once and
  // handed to the operating system; it is never read back, so the field stays
  // blank afterwards and an empty box means "leave the stored one alone".
  const [cloud, setCloud] = useState<{
    enabled: boolean; configured: boolean; credential_store: boolean;
    every_days: number; last_success: string | null; last_error: string | null;
    days_since_success: number | null; needs_manual_backup: boolean;
  } | null>(null);
  const [cloudForm, setCloudForm] = useState({
    client_id: '', client_secret: '', folder_id: '', refresh_token: '',
    accounts_base: 'https://accounts.mgovcloud.in',
    api_base: 'https://workdrive.mgovcloud.in/api/v1',
    every_days: '30',
  });
  const [cloudBusy, setCloudBusy] = useState(false);
  const [cloudMsg, setCloudMsg] = useState('');

  const loadCloud = useCallback(async () => {
    try {
      const r = await api.get('/backup/cloud/status', { headers: adminHeaders(adminToken) });
      setCloud(r.data);
    } catch { /* the panel simply shows nothing about it */ }
  }, [adminToken]);

  const saveCloud = async (enabled: boolean) => {
    setCloudBusy(true); setCloudMsg('');
    try {
      const body: Record<string, unknown> = {
        enabled,
        accounts_base: cloudForm.accounts_base,
        api_base:      cloudForm.api_base,
        every_days:    Number(cloudForm.every_days) || 30,
      };
      // Only what was typed is sent: an untouched box must not blank a setting
      // that is already there.
      for (const k of ['client_id', 'client_secret', 'folder_id', 'refresh_token'] as const) {
        if (cloudForm[k].trim()) body[k] = cloudForm[k].trim();
      }
      const r = await api.post('/admin/backup/cloud/settings', body,
                               { headers: adminHeaders(adminToken) });
      setCloud(r.data);
      setCloudForm(f => ({ ...f, client_secret: '', refresh_token: '' }));
      setCloudMsg(enabled ? 'Saved. Use "Send one now" to prove it works.' : 'Turned off.');
    } catch (err: unknown) {
      setCloudMsg((err as { response?: { data?: { detail?: string } } })?.response?.data?.detail
                  || 'Could not save.');
    } finally { setCloudBusy(false); }
  };

  const runCloudNow = async () => {
    setCloudBusy(true); setCloudMsg('Sending…');
    try {
      const r = await api.post('/admin/backup/cloud/run', {},
                               { headers: adminHeaders(adminToken) });
      setCloudMsg(`Sent — ${Number(r.data.bytes || 0).toLocaleString('en-IN')} bytes.`);
      await loadCloud();
    } catch (err: unknown) {
      setCloudMsg((err as { response?: { data?: { detail?: string } } })?.response?.data?.detail
                  || 'The copy could not be sent.');
      await loadCloud();
    } finally { setCloudBusy(false); }
  };

  // App mode
  const [prodMode, setProdMode] = useState(false);

  // The day the office starts keeping cases the new way; before it, legacy.
  const [legacyCutoff, setLegacyCutoff] = useState('');
  const [cutoffSaving, setCutoffSaving] = useState(false);
  const [cutoffMsg, setCutoffMsg] = useState('');

  // Allowed devices (IP/MAC whitelist)
  const [devices, setDevices] = useState<AllowedDeviceRow[]>([]);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [showAddDevice, setShowAddDevice] = useState(false);
  const [newDevice, setNewDevice] = useState({ label: '', ip_address: '', mac_address: '', hostname: '', notes: '' });
  const [addDeviceErr, setAddDeviceErr] = useState('');
  const [addDeviceMsg, setAddDeviceMsg] = useState('');
  const [deviceMsg, setDeviceMsg] = useState('');

  // Compact backup — the recommended one. Written straight to disk by the
  // backend, so nothing is buffered in the page and size stops mattering.
  const [compactLoading, setCompactLoading] = useState(false);
  const [compactMsg, setCompactMsg] = useState('');
  const [compactErr, setCompactErr] = useState(false);
  const [autoStatus, setAutoStatus] = useState<AutoBackupStatus | null>(null);

  // Backup download
  const [backupLoading, setBackupLoading] = useState(false);
  const [backupMsg, setBackupMsg] = useState('');
  const [backupProgress, setBackupProgress] = useState('');

  // Legacy CSV upload — cops_master (from old MDB)
  const legacyRef = useRef<HTMLInputElement>(null);
  const [legacyFile, setLegacyFile] = useState<File | null>(null);
  const [legacyLoading, setLegacyLoading] = useState(false);
  const [legacyResult, setLegacyResult] = useState('');
  const [legacyErr, setLegacyErr] = useState('');

  // Legacy CSV upload — cops_items (from old MDB)
  const legacyItemsRef = useRef<HTMLInputElement>(null);
  const [legacyItemsFile, setLegacyItemsFile] = useState<File | null>(null);
  const [legacyItemsLoading, setLegacyItemsLoading] = useState(false);
  const [legacyItemsResult, setLegacyItemsResult] = useState('');
  const [legacyItemsErr, setLegacyItemsErr] = useState('');

  // MDB direct import (file upload)
  const [mdbFile, setMdbFile] = useState<File | null>(null);
  const [mdbLoading, setMdbLoading] = useState(false);
  const [mdbResult, setMdbResult] = useState('');
  const [mdbErr, setMdbErr] = useState('');

  // Restore from backup ZIP
  const restoreRef = useRef<HTMLInputElement>(null);
  const [restoreFile, setRestoreFile] = useState<File | null>(null);
  const [restoreLoading, setRestoreLoading] = useState(false);
  const [restoreResult, setRestoreResult] = useState('');
  const [restoreErr, setRestoreErr] = useState('');

  // Full SQLite DB backup / restore

  // Tab navigation
  const [activeTab, setActiveTab] = useState<'security' | 'users' | 'settings' | 'backup' | 'osconfig' | 'statutes' | 'danger'>('security');

  // Purge OS case
  const [purgeOsNo, setPurgeOsNo] = useState('');
  const [purgeYear, setPurgeYear] = useState('');
  const [purgePwd, setPurgePwd] = useState('');
  const [showPurgePwd, setShowPurgePwd] = useState(false);
  const [purgeConfirmed, setPurgeConfirmed] = useState(false);
  const [purgeLoading, setPurgeLoading] = useState(false);
  const [purgeResult, setPurgeResult] = useState<{ message: string; total_rows_deleted: number; breakdown: Record<string, number> } | null>(null);
  const [purgeErr, setPurgeErr] = useState('');

  // ── Load on login ─────────────────────────────────────────────────────────
  useEffect(() => {
    if (!adminToken) return;
    reloadTrial().catch(() => setTrialStatus(null));
    loadUsers();
    api.get('/admin/features', { headers: adminHeaders(adminToken) })
      .then(r => setApisEnabled(r.data.apis_enabled === true || r.data.apis_enabled === 'true')).catch(() => {});
    loadCloud();
    api.get('/config/legacy-cutoff', { headers: adminHeaders(adminToken) })
      .then(r => setLegacyCutoff(r.data.cutoff || '')).catch(() => {});
    api.get('/admin/mode', { headers: adminHeaders(adminToken) })
      // `mode` is the module this installation runs as; `prod_mode` is whether
      // this is an installed build. Reading the first as the second labelled
      // every SDO machine "Development Mode" for good.
      .then(r => { setProdMode(r.data.prod_mode === true); })
      .catch(() => {});
    loadDevices();
  }, [adminToken]);

  // ── Allowed Devices ───────────────────────────────────────────────────────
  function loadDevices() {
    setDevicesLoading(true);
    api.get('/admin/devices', { headers: adminHeaders(adminToken) })
      .then(r => setDevices(r.data))
      .catch(() => {})
      .finally(() => setDevicesLoading(false));
  }

  const addDevice = async (e: React.FormEvent) => {
    e.preventDefault();
    setAddDeviceErr('');
    setAddDeviceMsg('');
    try {
      await api.post('/admin/devices', {
        label: newDevice.label,
        ip_address: newDevice.ip_address || null,
        mac_address: newDevice.mac_address || null,
        hostname: newDevice.hostname || null,
        notes: newDevice.notes || null,
      }, { headers: adminHeaders(adminToken) });
      setAddDeviceMsg(`Device '${newDevice.label}' added.`);
      setNewDevice({ label: '', ip_address: '', mac_address: '', hostname: '', notes: '' });
      setShowAddDevice(false);
      loadDevices();
    } catch (err: any) {
      setAddDeviceErr(err.response?.data?.detail || 'Failed to add device.');
    }
  };

  const removeDevice = async (id: number, label: string) => {
    if (!window.confirm(`Remove '${label}' from the whitelist?`)) return;
    try {
      await api.delete(`/admin/devices/${id}`, { headers: adminHeaders(adminToken) });
      setDeviceMsg(`'${label}' removed.`);
      loadDevices();
    } catch (err: any) {
      setDeviceMsg(err.response?.data?.detail || 'Failed to remove device.');
    }
  };

  const toggleDeviceActive = async (id: number, label: string, currentActive: boolean) => {
    try {
      await api.put(`/admin/devices/${id}`, { is_active: !currentActive }, { headers: adminHeaders(adminToken) });
      setDeviceMsg(`'${label}' ${!currentActive ? 'enabled' : 'disabled'}.`);
      loadDevices();
    } catch (err: any) {
      setDeviceMsg(err.response?.data?.detail || 'Failed to update device.');
    }
  };

  const toggleApis = async (enable: boolean) => {
    setFlagsLoading(true);
    setFlagsMsg('');
    try {
      await api.put('/admin/features', { apis_enabled: enable ? 'true' : 'false' }, { headers: adminHeaders(adminToken) });
      setApisEnabled(enable);
      setFlagsMsg(enable ? 'COPS ↔ APIS module enabled.' : 'COPS ↔ APIS module disabled.');
    } catch (err: any) {
      setFlagsMsg(err.response?.data?.detail || 'Failed to update feature flag.');
    } finally {
      setFlagsLoading(false);
    }
  };

  function loadUsers() {
    setUsersLoading(true);
    api.get('/admin/users', { headers: adminHeaders(adminToken) })
      .then(r => setUsers(r.data))
      .catch(() => {})
      .finally(() => setUsersLoading(false));
  }

  // ── Admin login ───────────────────────────────────────────────────────────
  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoginErr('');
    setLoginLoading(true);
    try {
      const res = await api.post('/admin/login', { username: loginUser, password: loginPwd });
      setAdminToken(res.data.access_token);
    } catch (err: any) {
      setLoginErr(err.response?.data?.detail || 'Login failed.');
    } finally {
      setLoginLoading(false);
    }
  };

  // ── Device registration ───────────────────────────────────────────────────
  const registerDevice = async () => {
    setRegLoading(true);
    setRegMsg('');
    try {
      // /admin/register-device is the one that registers THIS machine.
      // /admin/devices adds an arbitrary row and answers with that row, so
      // posting there left the panel holding a shape with no `registered` field
      // — the machine really was authorised and the screen went on saying it
      // was not. Read the status back from the endpoint that reports it.
      await api.post('/admin/register-device', {}, { headers: adminHeaders(adminToken) });
      const info = await api.get('/admin/device-info', { headers: adminHeaders(adminToken) });
      setDeviceInfo(info.data);
      setRegMsg('Device registered. This machine is now authorised.');
    } catch (err: any) {
      setRegMsg(err.response?.data?.detail || 'Registration failed.');
    } finally {
      setRegLoading(false);
    }
  };

  // ── Create user ───────────────────────────────────────────────────────────
  const createUser = async (e: React.FormEvent) => {
    e.preventDefault();
    setCreateErr('');
    setCreateMsg('');
    try {
      // Backend CreateUserRequest expects 'password', not 'user_pwd'
      await api.post('/admin/users', {
        user_name: newUser.user_name,
        user_desig: newUser.user_desig || null,
        user_id: newUser.user_id,
        password: newUser.user_pwd,
        user_role: newUser.user_role,
      }, { headers: adminHeaders(adminToken) });
      setCreateMsg(`User '${newUser.user_id}' created.`);
      setNewUser({ user_id: '', user_name: '', user_desig: '', user_pwd: '', user_role: 'SDO' });
      setShowCreate(false);
      loadUsers();
    } catch (err: any) {
      setCreateErr(err.response?.data?.detail || 'Create failed.');
    }
  };

  // ── Update user ───────────────────────────────────────────────────────────
  const saveEdit = async (e: React.FormEvent) => {
    e.preventDefault();
    setEditMsg('');
    const payload: any = {};
    if (editData.user_name)   payload.user_name   = editData.user_name;
    if (editData.user_desig)  payload.user_desig  = editData.user_desig;
    if (editData.user_pwd)    payload.password    = editData.user_pwd;
    if (editData.user_role)   payload.user_role   = editData.user_role;
    if (editData.user_status) payload.user_status = editData.user_status;
    try {
      // Backend uses numeric id (Path<i64>), not user_id string
      await api.patch(`/admin/users/${editId}`, payload, { headers: adminHeaders(adminToken) });
      setEditMsg('Saved.');
      setEditId(null);
      loadUsers();
    } catch (err: any) {
      setEditMsg(err.response?.data?.detail || 'Update failed.');
    }
  };

  const closeUser = async (id: number) => {
    if (!confirm(`Close this account?`)) return;
    try {
      await api.delete(`/admin/users/${id}`, { headers: adminHeaders(adminToken) });
      loadUsers();
    } catch { }
  };

  const hardDeleteUser = async (id: number) => {
    if (!confirm(`Permanently delete this account?\n\nThis completely removes it from the list. (OS records created by this user will remain safe).`)) return;
    try {
      await api.delete(`/admin/users/${id}/hard`, { headers: adminHeaders(adminToken) });
      loadUsers();
    } catch (err: any) {
      alert(err.response?.data?.detail || 'Failed to delete user.');
    }
  };

  // ── Compact backup ────────────────────────────────────────────────────────
  // The officer picks the file; the BACKEND writes it. Deliberately not an HTTP
  // download: that route holds the whole archive in the page as a blob and then
  // copies it again into an ArrayBuffer before writing, which is what broke the
  // old full-database download once the database grew.
  const saveCompactBackup = async () => {
    setCompactLoading(true);
    setCompactMsg('');
    setCompactErr(false);
    try {
      const defaultName = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
      let savePath: string | null = null;
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        savePath = await save({
          title: 'Save COPS Backup',
          defaultPath: defaultName,
          filters: [{ name: 'COPS Backup', extensions: ['cops'] }],
        });
      } catch {
        // Running in a plain browser rather than the desktop shell — fall back
        // to the streaming download, which is correct there.
        window.location.href = `${api.defaults.baseURL}/backup/archive/download`;
        setCompactLoading(false);
        return;
      }
      if (!savePath) { setCompactMsg('Save cancelled.'); setCompactLoading(false); return; }

      setCompactMsg('Building the backup…');
      // Must carry the ADMIN token explicitly. This page is reachable without an
      // officer being signed in, so the interceptor's localStorage token may not
      // exist — the call would 401 and the response interceptor would clear the
      // session token as a side effect.
      const res = await api.post('/backup/archive/save', { path: savePath },
        { timeout: 0, headers: adminHeaders(adminToken) });
      setCompactMsg(res.data.message || 'Backup saved.');
      showDownloadToast(`Backup saved to ${savePath}`);
      loadAutoStatus();
    } catch (err: unknown) {
      setCompactErr(true);
      setCompactMsg((err as any)?.response?.data?.detail || (err instanceof Error ? err.message : String(err)));
    } finally {
      setCompactLoading(false);
    }
  };

  const loadAutoStatus = async () => {
    try {
      const r = await api.get('/backup/auto/status', { headers: adminHeaders(adminToken) });
      setAutoStatus(r.data);
    } catch { /* status is informational; never block the page on it */ }
  };

  useEffect(() => { if (activeTab === 'backup' && adminToken) loadAutoStatus(); }, [activeTab, adminToken]);

  // ── Backup download ───────────────────────────────────────────────────────
  // The one backup: the whole register in a single compressed, encrypted file.
  //
  // It used to be a ZIP of CSVs of only settings and cases — a second, weaker
  // kind of backup that invited the officer to save the wrong thing. There is
  // one backup now, and this is it: every table, about six times smaller than a
  // copy of the database, openable only with the office's own key.
  const downloadBackup = async () => {
    setBackupLoading(true); setBackupMsg(''); setBackupProgress('Preparing…');
    const name = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
    try {
      // Preferred path: the server writes straight to the folder the officer
      // chooses, so the size never has to fit in memory.
      let savePath: string | null = null;
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        savePath = await save({
          title: 'Save the backup',
          defaultPath: name,
          filters: [{ name: 'COPS backup', extensions: ['cops'] }],
        });
      } catch { /* not in the desktop window — fall through to a download */ }

      if (savePath) {
        setBackupProgress('Writing…');
        await api.post('/backup/archive/save', { path: savePath },
                       { headers: adminHeaders(adminToken), timeout: 0 });
        setBackupProgress('');
        setBackupMsg(`Backup saved to ${savePath}`);
        showDownloadToast('Backup saved — one encrypted file with every record.');
      } else {
        // In a browser: stream it down.
        const res = await api.get('/backup/archive/download', {
          headers: adminHeaders(adminToken), responseType: 'blob', timeout: 0,
        });
        const url = URL.createObjectURL(res.data as Blob);
        const a = document.createElement('a');
        a.href = url; a.download = name;
        document.body.appendChild(a); a.click(); a.remove();
        setTimeout(() => URL.revokeObjectURL(url), 60_000);
        setBackupProgress('');
        setBackupMsg('Backup downloaded.');
      }
    } catch (e: unknown) {
      setBackupProgress('');
      const d = (e as { response?: { data?: { detail?: string } }; message?: string });
      setBackupMsg(`Backup failed: ${d?.response?.data?.detail || d?.message || 'unknown error'}.`);
    } finally {
      setBackupLoading(false);
    }
  };

  // ── Full DB export ────────────────────────────────────────────────────────


  // ── Legacy CSV upload — cops_master ──────────────────────────────────────
  const uploadLegacy = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!legacyFile) return;
    setLegacyErr('');
    setLegacyResult('');
    setLegacyLoading(true);
    const fd = new FormData();
    fd.append('file', legacyFile);
    try {
      const res = await api.post('/admin/backup/upload-legacy', fd, {
        headers: { ...adminHeaders(adminToken), 'Content-Type': 'multipart/form-data' },
      });
      const { inserted, skipped, invalid } = res.data;
      setLegacyResult(`Done — ${inserted} cases inserted, ${skipped} skipped (already exist), ${invalid} invalid rows.`);
      setLegacyFile(null);
      if (legacyRef.current) legacyRef.current.value = '';
    } catch (err: any) {
      setLegacyErr(err.response?.data?.detail || 'Upload failed.');
    } finally {
      setLegacyLoading(false);
    }
  };

  // ── Legacy CSV upload — cops_items ────────────────────────────────────────
  const uploadLegacyItems = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!legacyItemsFile) return;
    setLegacyItemsErr('');
    setLegacyItemsResult('');
    setLegacyItemsLoading(true);
    const fd = new FormData();
    fd.append('file', legacyItemsFile);
    try {
      const res = await api.post('/admin/backup/upload-legacy-items', fd, {
        headers: { ...adminHeaders(adminToken), 'Content-Type': 'multipart/form-data' },
      });
      const { inserted, skipped, invalid } = res.data;
      setLegacyItemsResult(`Done — ${inserted} items inserted, ${skipped} skipped (already exist), ${invalid} invalid rows.`);
      setLegacyItemsFile(null);
      if (legacyItemsRef.current) legacyItemsRef.current.value = '';
    } catch (err: any) {
      setLegacyItemsErr(err.response?.data?.detail || 'Upload failed.');
    } finally {
      setLegacyItemsLoading(false);
    }
  };

  // ── MDB direct import ─────────────────────────────────────────────────────
  const importMdb = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!mdbFile) return;
    setMdbErr('');
    setMdbResult('');
    setMdbLoading(true);
    const fd = new FormData();
    fd.append('file', mdbFile);
    try {
      const res = await api.post('/admin/backup/import-mdb', fd, {
        headers: { ...adminHeaders(adminToken), 'Content-Type': 'multipart/form-data' },
        timeout: 600000, // 10 min — large file
      });
      const d = res.data;
      const lines: string[] = [
        `OS: ${d.master_inserted} inserted, ${d.master_skipped} skipped, ${d.master_invalid} invalid — Items: ${d.items_inserted} ins / ${d.items_skipped} skip`,
      ];
      if ((d.br_inserted ?? 0) + (d.br_skipped ?? 0) + (d.br_invalid ?? 0) > 0)
        lines.push(`BR: ${d.br_inserted} inserted, ${d.br_skipped} skipped, ${d.br_invalid} invalid — Items: ${d.br_items_inserted} ins / ${d.br_items_skipped} skip`);
      if ((d.dr_inserted ?? 0) + (d.dr_skipped ?? 0) + (d.dr_invalid ?? 0) > 0)
        lines.push(`DR: ${d.dr_inserted} inserted, ${d.dr_skipped} skipped, ${d.dr_invalid} invalid — Items: ${d.dr_items_inserted} ins / ${d.dr_items_skipped} skip`);
      setMdbResult(`Done — ${lines.join(' | ')}`);
    } catch (err: any) {
      setMdbErr(err.response?.data?.detail || 'Import failed. Check the file and try again.');
    } finally {
      setMdbLoading(false);
    }
  };

  // A CSV export for moving selected data between machines — not a backup.
  //
  // Kept for the admin who is migrating or reconciling data by hand. It is
  // deliberately not called a backup and not offered to officers: it holds only
  // cases and settings as CSV, not the whole encrypted register, and confusing
  // the two is how someone saves the wrong thing.
  const [csvBusy, setCsvBusy] = useState(false);
  const [csvMsg, setCsvMsg] = useState('');
  const downloadCsvMigration = async () => {
    setCsvBusy(true); setCsvMsg('');
    try {
      const res = await api.get('/admin/backup/export',
        { headers: adminHeaders(adminToken), responseType: 'blob', timeout: 0 });
      const url = URL.createObjectURL(res.data as Blob);
      const a = document.createElement('a');
      a.href = url; a.download = `cops_cases_export_${new Date().toISOString().slice(0, 10)}.zip`;
      document.body.appendChild(a); a.click(); a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
      setCsvMsg('Exported.');
    } catch (e: unknown) {
      setCsvMsg((e as { response?: { data?: { detail?: string } } })?.response?.data?.detail
                || 'Export failed.');
    } finally { setCsvBusy(false); }
  };

  // ── Restore, whatever kind of backup it is ──────────────────────────────────
  //
  // One box. The current backup is a .cops archive; a .db or a .zip is an older
  // one the office might still have on a drive. The right path is chosen from the
  // file itself, so recovery is: pick the file, confirm, done.
  const uploadRestore = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!restoreFile) return;
    const nm = restoreFile.name.toLowerCase();
    const kind = nm.endsWith('.cops') ? 'archive'
               : nm.endsWith('.db')   ? 'fulldb'
               : nm.endsWith('.zip')  ? 'merge'
               : '';
    if (!kind) {
      setRestoreErr('Choose a .cops backup (or an older .db or .zip).');
      return;
    }
    if (kind !== 'merge' && !window.confirm(
      'This replaces ALL current data with the backup. It cannot be undone.\n\nProceed?'
    )) return;

    setRestoreErr(''); setRestoreResult(''); setRestoreLoading(true);
    try {
      const post = async (confirmLoss: boolean) => {
        const fd = new FormData();
        fd.append('file', restoreFile);
        if (confirmLoss) fd.append('confirm_data_loss', 'yes');
        const url = kind === 'archive' ? '/admin/backup/restore-archive'
                  : kind === 'fulldb'  ? '/admin/backup/restore-fulldb'
                  : '/admin/backup/restore';
        return api.post(url, fd, {
          headers: { ...adminHeaders(adminToken), 'Content-Type': 'multipart/form-data' },
          timeout: 600000,
        });
      };

      let res;
      try {
        res = await post(false);
      } catch (err: unknown) {
        // The archive restore refuses, unasked, to overwrite good data with a
        // backup that holds fewer records. If the officer means it, they say so.
        const detail = (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail || '';
        if (kind === 'archive' && /fewer records/i.test(detail)
            && window.confirm(detail + '\n\nRestore anyway?')) {
          res = await post(true);
        } else {
          throw err;
        }
      }

      const d = res.data;
      setRestoreResult(d.message
        || (typeof d === 'object' ? 'Restored. Restart the app to reload the data.' : String(d)));
      setRestoreFile(null);
      if (restoreRef.current) restoreRef.current.value = '';
    } catch (err: unknown) {
      setRestoreErr((err as { response?: { data?: { detail?: string } } })?.response?.data?.detail
                    || 'Restore failed. The file may not be a COPS backup, or the key is wrong.');
    } finally {
      setRestoreLoading(false);
    }
  };

  // ── Login screen ──────────────────────────────────────────────────────────
  if (!adminToken) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-slate-100">
        <div className="w-full max-w-sm bg-white rounded-xl shadow-xl border border-slate-200 p-8 space-y-6">
          <div className="flex flex-col items-center gap-2">
            <div className="p-3 bg-slate-800 rounded-xl">
              <Lock size={28} className="text-white" />
            </div>
            <h1 className="text-lg font-bold text-slate-800">System Admin</h1>
            <p className="text-xs text-slate-500 text-center">Restricted to the system administrator only.</p>
          </div>

          <form onSubmit={handleLogin} className="space-y-4">
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Username</label>
              <input
                type="text" required autoComplete="off"
                value={loginUser} onChange={e => setLoginUser(e.target.value)}
                className="w-full border border-slate-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-slate-400"
                placeholder="Admin username"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Password</label>
              <div className="relative">
                <input
                  type={showLoginPwd ? 'text' : 'password'} required
                  value={loginPwd} onChange={e => setLoginPwd(e.target.value)}
                  className="w-full border border-slate-200 rounded-lg px-3 py-2 pr-9 text-sm focus:outline-none focus:ring-2 focus:ring-slate-400"
                  placeholder="Admin password"
                />
                <button type="button" tabIndex={-1}
                  onClick={() => setShowLoginPwd(v => !v)}
                  className="absolute right-2 top-2 text-slate-400 hover:text-slate-600">
                  {showLoginPwd ? <EyeOff size={15} /> : <Eye size={15} />}
                </button>
              </div>
            </div>
            {loginErr && <p className="text-xs text-red-600">{loginErr}</p>}
            <button type="submit" disabled={loginLoading}
              className="w-full py-2 rounded-lg bg-slate-800 text-white text-sm font-semibold hover:bg-slate-700 disabled:opacity-60 flex items-center justify-center gap-2">
              <LogIn size={15} />
              {loginLoading ? 'Verifying…' : 'Sign In as Admin'}
            </button>
          </form>

          <button onClick={() => navigate('/modules')}
            className="w-full text-xs text-slate-400 hover:text-slate-700 flex items-center justify-center gap-1">
            <ArrowLeft size={13} /> Back to Module Selection
          </button>
        </div>
      </div>
    );
  }

  // ── Admin dashboard ───────────────────────────────────────────────────────
  const TABS = [
    { id: 'security' as const, label: 'Security',        icon: <ShieldCheck size={14} /> },
    { id: 'users'    as const, label: 'Users',           icon: <Users size={14} /> },
    { id: 'settings' as const, label: 'Settings',        icon: <ScanLine size={14} /> },
    { id: 'backup'   as const, label: 'Backup & Restore',icon: <Database size={14} /> },
    { id: 'osconfig' as const, label: 'OS Config',       icon: <Settings size={14} /> },
    { id: 'statutes' as const, label: 'Legal Statutes',  icon: <Scale size={14} /> },
    { id: 'danger'   as const, label: 'Danger Zone',     icon: <Trash2 size={14} className="text-red-500" /> },
  ];

  const handlePurgeOS = async (e: React.FormEvent) => {
    e.preventDefault();
    setPurgeErr('');
    setPurgeResult(null);
    if (!purgeOsNo.trim() || !purgeYear.trim() || !purgePwd) {
      setPurgeErr('All fields are required.');
      return;
    }
    if (!purgeConfirmed) {
      setPurgeErr('You must check the confirmation box before proceeding.');
      return;
    }
    setPurgeLoading(true);
    try {
      const res = await api.post('/admin/purge-os', {
        os_no: purgeOsNo.trim(),
        os_year: parseInt(purgeYear, 10),
        admin_password: purgePwd,
      }, { headers: adminHeaders(adminToken) });
      setPurgeResult(res.data);
      setPurgeOsNo('');
      setPurgeYear('');
      setPurgePwd('');
      setPurgeConfirmed(false);
    } catch (err: any) {
      setPurgeErr(err.response?.data?.detail || 'Purge failed. Please try again.');
    } finally {
      setPurgeLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-slate-100">
    <div className="max-w-4xl mx-auto py-6 px-4 space-y-5 pb-12">

      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-slate-800 rounded-lg"><Lock size={18} className="text-white" /></div>
          <div>
            <h1 className="text-lg font-bold text-slate-800">System Admin Panel</h1>
            <p className="text-xs text-slate-500 flex items-center gap-1.5">
              <span className={`inline-block w-1.5 h-1.5 rounded-full ${prodMode ? 'bg-emerald-500' : 'bg-amber-400'}`} />
              {prodMode ? 'Production Mode' : 'Development Mode'}
            </p>
          </div>
        </div>
        <div className="flex gap-2">
          <button onClick={() => navigate('/modules')}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md border border-slate-200 text-slate-600 hover:bg-slate-100">
            <ArrowLeft size={13} /> Back
          </button>
          <button onClick={() => setAdminToken('')}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-slate-100 text-slate-700 hover:bg-slate-200">
            Sign Out
          </button>
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex gap-1 bg-slate-100 rounded-xl p-1">
        {TABS.map(tab => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-1.5 flex-1 justify-center px-3 py-2 rounded-lg text-xs font-medium transition-colors ${
              activeTab === tab.id
                ? 'bg-white text-slate-800 shadow-sm'
                : 'text-slate-500 hover:text-slate-700'
            }`}
          >
            {tab.icon}
            {tab.label}
          </button>
        ))}
      </div>

      {/* ══ TAB: SECURITY ══════════════════════════════════════════════════════ */}
      {activeTab === 'security' && (
        <div className="space-y-5">

          {/* Device Authorisation */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
            <div className="flex items-center gap-2">
              {deviceInfo?.registered
                ? <ShieldCheck size={18} className="text-emerald-600" />
                : <ShieldAlert size={18} className="text-amber-500" />}
              <h2 className="text-sm font-semibold text-slate-700">Device Authorisation</h2>
              <span className="text-xs text-slate-400 ml-auto">Master terminal only</span>
            </div>
            {deviceInfo && (
              <div className="font-mono text-xs bg-slate-50 rounded-lg p-3 space-y-1 border border-slate-100">
                <div><span className="text-slate-400">Hostname : </span>{deviceInfo.hostname}</div>
                <div><span className="text-slate-400">MAC      : </span>{deviceInfo.mac}</div>
                <div>
                  <span className="text-slate-400">Status   : </span>
                  {deviceInfo.registered
                    ? <span className="text-emerald-600 font-bold">Authorised ✓</span>
                    : <span className="text-amber-600 font-bold">Not registered</span>}
                </div>
              </div>
            )}
            {!deviceInfo?.registered && (
              <p className="text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded p-2">
                This device is not authorised. In Production mode, all data endpoints are blocked until registered.
              </p>
            )}
            {!deviceInfo?.registered && (
              <button disabled={regLoading} onClick={registerDevice}
                className="px-4 py-1.5 text-xs rounded-lg bg-amber-600 text-white hover:bg-amber-700 disabled:opacity-60">
                {regLoading ? 'Registering…' : 'Register This Device'}
              </button>
            )}
            {regMsg && <p className="text-xs text-emerald-700">{regMsg}</p>}
          </section>

          {/* Network Access Control */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
            <div className="flex items-center gap-2">
              <Wifi size={18} className="text-slate-600" />
              <h2 className="text-sm font-semibold text-slate-700">Network Access Control</h2>
              <button
                onClick={() => { setShowAddDevice(v => !v); setAddDeviceErr(''); setAddDeviceMsg(''); }}
                className="ml-auto flex items-center gap-1 px-3 py-1 bg-slate-800 text-white text-xs rounded-lg hover:bg-slate-700 transition-colors"
              >
                <Plus size={13} /> Add Device
              </button>
            </div>

            <div className="flex items-start gap-2 bg-blue-50 border border-blue-200 rounded-lg p-3 text-xs text-blue-800">
              <ShieldCheck size={14} className="mt-0.5 shrink-0" />
              <span>
                <strong>127.0.0.1 (this master terminal) is always allowed</strong> regardless of this list.
                Whitelist is only enforced when <code className="bg-blue-100 px-1 rounded">COPS_ENV=production</code> is set.
                IP is enforced; MAC is stored for identification only.
              </span>
            </div>

            {showAddDevice && (
              <form onSubmit={addDevice} className="border border-slate-200 rounded-lg p-4 space-y-3 bg-slate-50">
                <p className="text-xs font-semibold text-slate-700">Add New Device</p>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-xs text-slate-500 mb-1 block">Label *</label>
                    <input required value={newDevice.label} onChange={e => setNewDevice(p => ({ ...p, label: e.target.value }))}
                      placeholder="Counter 1 - Immigration"
                      className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" />
                  </div>
                  <div>
                    <label className="text-xs text-slate-500 mb-1 block">IP Address</label>
                    <input value={newDevice.ip_address} onChange={e => setNewDevice(p => ({ ...p, ip_address: e.target.value }))}
                      placeholder="192.168.1.101"
                      className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" />
                  </div>
                  <div>
                    <label className="text-xs text-slate-500 mb-1 block">MAC Address</label>
                    <input value={newDevice.mac_address} onChange={e => setNewDevice(p => ({ ...p, mac_address: e.target.value }))}
                      placeholder="AA:BB:CC:DD:EE:FF"
                      className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" />
                  </div>
                  <div>
                    <label className="text-xs text-slate-500 mb-1 block">Hostname</label>
                    <input value={newDevice.hostname} onChange={e => setNewDevice(p => ({ ...p, hostname: e.target.value }))}
                      placeholder="WORKSTATION-01"
                      className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" />
                  </div>
                </div>
                <div>
                  <label className="text-xs text-slate-500 mb-1 block">Notes</label>
                  <input value={newDevice.notes} onChange={e => setNewDevice(p => ({ ...p, notes: e.target.value }))}
                    placeholder="Optional notes"
                    className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" />
                </div>
                {addDeviceErr && <p className="text-xs text-red-600">{addDeviceErr}</p>}
                {addDeviceMsg && <p className="text-xs text-emerald-600">{addDeviceMsg}</p>}
                <div className="flex gap-2">
                  <button type="submit" className="px-3 py-1.5 bg-emerald-600 text-white text-xs rounded hover:bg-emerald-700">Add</button>
                  <button type="button" onClick={() => setShowAddDevice(false)} className="px-3 py-1.5 bg-slate-200 text-slate-700 text-xs rounded hover:bg-slate-300">Cancel</button>
                </div>
              </form>
            )}

            {devicesLoading ? (
              <p className="text-xs text-slate-400">Loading devices...</p>
            ) : devices.length === 0 ? (
              <p className="text-xs text-slate-400 italic">No devices whitelisted yet. In Production mode, all LAN clients except this terminal will be blocked.</p>
            ) : (
              <div className="overflow-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-slate-200 text-slate-500 text-left">
                      <th className="pb-2 font-medium">Label</th>
                      <th className="pb-2 font-medium">IP Address</th>
                      <th className="pb-2 font-medium">MAC</th>
                      <th className="pb-2 font-medium">Hostname</th>
                      <th className="pb-2 font-medium">Status</th>
                      <th className="pb-2 font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {devices.map(d => (
                      <tr key={d.id} className={d.is_active ? '' : 'opacity-50'}>
                        <td className="py-2 font-medium text-slate-700">{d.label}</td>
                        <td className="py-2 font-mono text-slate-600">{d.ip_address || '—'}</td>
                        <td className="py-2 font-mono text-slate-500">{d.mac_address || '—'}</td>
                        <td className="py-2 text-slate-500">{d.hostname || '—'}</td>
                        <td className="py-2">
                          <span className={`px-1.5 py-0.5 rounded text-xs font-medium ${d.is_active ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-100 text-slate-500'}`}>
                            {d.is_active ? 'Active' : 'Inactive'}
                          </span>
                        </td>
                        <td className="py-2 flex gap-2">
                          <button onClick={() => toggleDeviceActive(d.id, d.label, d.is_active)}
                            className="text-slate-500 hover:text-slate-700" title={d.is_active ? 'Disable' : 'Enable'}>
                            {d.is_active ? <ToggleRight size={14} /> : <ToggleLeft size={14} />}
                          </button>
                          <button onClick={() => removeDevice(d.id, d.label)}
                            className="text-red-400 hover:text-red-600" title="Remove">
                            <X size={14} />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {deviceMsg && <p className="text-xs text-emerald-700">{deviceMsg}</p>}
          </section>
        </div>
      )}

      {/* ══ TAB: USERS ══════════════════════════════════════════════════════════ */}
      {activeTab === 'users' && (
        <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Users size={18} className="text-slate-600" />
              <h2 className="text-sm font-semibold text-slate-700">User Management</h2>
            </div>
            <div className="flex gap-2">
              <button onClick={loadUsers} className="p-1.5 rounded-lg border border-slate-200 hover:bg-slate-50">
                <RefreshCw size={13} className="text-slate-500" />
              </button>
              <button onClick={() => setShowCreate(v => !v)}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg bg-slate-800 text-white hover:bg-slate-700">
                <UserPlus size={13} />
                {showCreate ? 'Cancel' : 'Add User'}
              </button>
            </div>
          </div>

          {showCreate && (
            <form onSubmit={createUser} className="bg-slate-50 rounded-lg border border-slate-200 p-4 space-y-3">
              <p className="text-xs font-semibold text-slate-700">New User</p>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs text-slate-500 mb-1">Login ID</label>
                  <input required value={newUser.user_id} onChange={e => setNewUser(p => ({ ...p, user_id: e.target.value }))}
                    className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" placeholder="user@example.com" />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-1">Full Name</label>
                  <input required value={newUser.user_name} onChange={e => setNewUser(p => ({ ...p, user_name: e.target.value }))}
                    className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" placeholder="Display name" />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-1">Designation</label>
                  <input value={newUser.user_desig} onChange={e => setNewUser(p => ({ ...p, user_desig: e.target.value }))}
                    className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs" placeholder="e.g. Superintendent" />
                </div>
                <div>
                  <label className="block text-xs text-slate-500 mb-1">Role</label>
                  <select required value={newUser.user_role} onChange={e => setNewUser(p => ({ ...p, user_role: e.target.value }))}
                    className="w-full border border-slate-200 rounded px-2 py-1.5 text-xs">
                    <option value="SDO">SDO — SDO Module</option>
                    <option value="DC">DC — Adjudication Module</option>
                    <option value="AC">AC — Adjudication Module</option>
                  </select>
                </div>
                <div className="col-span-2">
                  <label className="block text-xs text-slate-500 mb-1">Initial Password</label>
                  <div className="relative">
                    <KeyRound size={12} className="absolute left-2 top-2 text-slate-400" />
                    <input required type={showNewPwd ? 'text' : 'password'}
                      value={newUser.user_pwd} onChange={e => setNewUser(p => ({ ...p, user_pwd: e.target.value }))}
                      className="w-full border border-slate-200 rounded pl-6 pr-8 py-1.5 text-xs" placeholder="Set initial password" />
                    <button type="button" tabIndex={-1} onClick={() => setShowNewPwd(v => !v)}
                      className="absolute right-2 top-1.5 text-slate-400 hover:text-slate-600">
                      {showNewPwd ? <EyeOff size={13} /> : <Eye size={13} />}
                    </button>
                  </div>
                </div>
              </div>
              {createErr && <p className="text-xs text-red-600">{createErr}</p>}
              {createMsg && <p className="text-xs text-emerald-700">{createMsg}</p>}
              <button type="submit" className="px-4 py-1.5 text-xs rounded-lg bg-slate-800 text-white hover:bg-slate-700">Create User</button>
            </form>
          )}

          {usersLoading ? (
            <p className="text-xs text-slate-400">Loading…</p>
          ) : users.length === 0 ? (
            <p className="text-xs text-slate-400 italic">No users yet.</p>
          ) : (
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-slate-100">
                    <th className="text-left py-2 px-2 text-slate-500 font-medium">Login ID</th>
                    <th className="text-left py-2 px-2 text-slate-500 font-medium">Name</th>
                    <th className="text-left py-2 px-2 text-slate-500 font-medium">Role</th>
                    <th className="text-left py-2 px-2 text-slate-500 font-medium">Status</th>
                    <th className="py-2 px-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {users.map(u => (
                    <Fragment key={u.user_id}>
                      <tr className="border-b border-slate-50 hover:bg-slate-50">
                        <td className="py-2 px-2 font-mono">{u.user_id}</td>
                        <td className="py-2 px-2">{u.user_name}</td>
                        <td className="py-2 px-2">
                          <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
                            u.user_role === 'SDO' ? 'bg-blue-100 text-blue-700' : 'bg-amber-100 text-amber-700'
                          }`}>{u.user_role}</span>
                        </td>
                        <td className="py-2 px-2">
                          <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
                            u.user_status === 'ACTIVE' ? 'bg-emerald-100 text-emerald-700' : 'bg-red-100 text-red-700'
                          }`}>{u.user_status}</span>
                        </td>
                        <td className="py-2 px-2 flex gap-1 justify-end">
                          <button onClick={() => {
                            setEditId(u.id);
                            setEditData({ user_name: u.user_name, user_desig: u.user_desig, user_pwd: '', user_role: u.user_role, user_status: u.user_status });
                            setEditMsg(''); setShowEditPwd(false);
                          }} className="p-1 rounded hover:bg-slate-100" title="Edit User">
                            <Pencil size={12} className="text-slate-500" />
                          </button>
                          {u.user_status === 'ACTIVE' ? (
                            <button onClick={() => closeUser(u.id)} className="p-1 rounded hover:bg-amber-50" title="Close Account">
                              <X size={12} className="text-amber-500" />
                            </button>
                          ) : (
                            <button onClick={() => hardDeleteUser(u.id)} className="p-1 rounded hover:bg-red-100" title="Permanently Delete">
                              <ShieldAlert size={12} className="text-red-700" />
                            </button>
                          )}
                        </td>
                      </tr>
                      {editId === u.id && (
                        <tr key={`edit-${u.user_id}`} className="bg-slate-50">
                          <td colSpan={5} className="px-2 py-3">
                            <form onSubmit={saveEdit} className="grid grid-cols-2 gap-2">
                              <input value={editData.user_name} onChange={e => setEditData(p => ({ ...p, user_name: e.target.value }))}
                                className="border border-slate-200 rounded px-2 py-1 text-xs" placeholder="Name" />
                              <input value={editData.user_desig} onChange={e => setEditData(p => ({ ...p, user_desig: e.target.value }))}
                                className="border border-slate-200 rounded px-2 py-1 text-xs" placeholder="Designation" />
                              <div className="relative">
                                <input type={showEditPwd ? 'text' : 'password'}
                                  value={editData.user_pwd} onChange={e => setEditData(p => ({ ...p, user_pwd: e.target.value }))}
                                  className="w-full border border-slate-200 rounded px-2 pr-7 py-1 text-xs" placeholder="New password (blank = keep)" />
                                <button type="button" tabIndex={-1} onClick={() => setShowEditPwd(v => !v)}
                                  className="absolute right-1.5 top-1 text-slate-400 hover:text-slate-600">
                                  {showEditPwd ? <EyeOff size={12} /> : <Eye size={12} />}
                                </button>
                              </div>
                              <select value={editData.user_role} onChange={e => setEditData(p => ({ ...p, user_role: e.target.value }))}
                                className="border border-slate-200 rounded px-2 py-1 text-xs">
                                {ALL_ROLES.map(r => <option key={r} value={r}>{r}</option>)}
                              </select>
                              <select value={editData.user_status} onChange={e => setEditData(p => ({ ...p, user_status: e.target.value }))}
                                className="border border-slate-200 rounded px-2 py-1 text-xs">
                                <option value="ACTIVE">ACTIVE</option>
                                <option value="CLOSED">CLOSED</option>
                              </select>
                              <div className="flex gap-2 items-center">
                                <button type="submit" className="px-3 py-1 text-xs rounded bg-slate-800 text-white hover:bg-slate-700">Save</button>
                                <button type="button" onClick={() => setEditId(null)} className="px-3 py-1 text-xs rounded bg-slate-200 hover:bg-slate-300">Cancel</button>
                                {editMsg && <span className="text-xs text-emerald-700">{editMsg}</span>}
                              </div>
                            </form>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      )}

      {/* ══ TAB: SETTINGS ═══════════════════════════════════════════════════════ */}
      {activeTab === 'settings' && (
        <div className="space-y-5">

          {/* App Mode — read-only indicator */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <Monitor size={18} className="text-slate-600" />
              <h2 className="text-sm font-semibold text-slate-700">App Mode</h2>
              <span className={`ml-auto text-xs font-bold px-2 py-0.5 rounded-full ${prodMode ? 'bg-emerald-100 text-emerald-700' : 'bg-amber-100 text-amber-700'}`}>
                {prodMode ? 'PRODUCTION' : 'DEVELOPMENT'}
              </span>
            </div>
            <div className={`flex items-start gap-2 rounded-lg p-3 text-xs border ${prodMode ? 'bg-emerald-50 border-emerald-200 text-emerald-800' : 'bg-amber-50 border-amber-200 text-amber-800'}`}>
              <AlertTriangle size={14} className="mt-0.5 shrink-0" />
              <span>
                {prodMode
                  ? 'Production mode is active. IP whitelist enforced, device registration required, login rate limiting on.'
                  : 'Development mode is active. All security gates are relaxed. To switch to Production, set COPS_ENV=production in the environment and restart the backend.'}
              </span>
            </div>

          </section>

          {/* Legacy cut-off — the day the office starts keeping cases the new way */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <CalendarDays size={18} className="text-blue-600" />
              <h2 className="text-sm font-semibold text-slate-700">Start of the new register</h2>
            </div>
            <p className="text-xs text-slate-500 leading-relaxed">
              Cases dated <strong>before</strong> this day are shown as <strong>Legacy O.S.</strong> —
              never as open or closed. Whether they were settled is not recorded anywhere this
              program can read, and it does not guess. From this day onwards a case is judged
              open or closed by what the revenue report shows collected against it.
              <br />
              It is a date, not a mark on each case: an offline sheet for August, loaded in
              September, lands on the right side of the line without anyone tagging it.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <input
                type="date"
                value={legacyCutoff}
                onChange={e => { setLegacyCutoff(e.target.value); setCutoffMsg(''); }}
                className="px-3 py-2 text-sm border border-slate-300 rounded-lg
                           focus:outline-none focus:ring-2 focus:ring-blue-400"
              />
              <button
                type="button"
                disabled={cutoffSaving}
                onClick={async () => {
                  setCutoffSaving(true); setCutoffMsg('');
                  try {
                    await api.put('/admin/config/legacy-cutoff', { cutoff: legacyCutoff },
                                  { headers: adminHeaders(adminToken) });
                    setCutoffMsg(legacyCutoff
                      ? `Saved. Cases before ${legacyCutoff} are shown as Legacy O.S.`
                      : 'Cleared. Every case is judged open or closed as usual.');
                  } catch (err: unknown) {
                    const d = (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail;
                    setCutoffMsg(d || 'Could not save the date.');
                  } finally { setCutoffSaving(false); }
                }}
                className="px-4 py-2 text-xs font-semibold rounded-lg bg-blue-600 text-white
                           hover:bg-blue-700 disabled:opacity-60"
              >
                {cutoffSaving ? 'Saving…' : 'Save'}
              </button>
              {legacyCutoff && (
                <button
                  type="button"
                  onClick={() => setLegacyCutoff('')}
                  className="px-3 py-2 text-xs rounded-lg border border-slate-300 text-slate-600 hover:bg-slate-50"
                >
                  Clear
                </button>
              )}
              {cutoffMsg && <span className="text-xs text-slate-600">{cutoffMsg}</span>}
            </div>
          </section>

          {/* Feature Flags */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
            <div className="flex items-center gap-2">
              <ScanLine size={18} className="text-violet-600" />
              <h2 className="text-sm font-semibold text-slate-700">Module Feature Flags</h2>
            </div>
            <p className="text-xs text-slate-500">
              Enable or disable optional modules across the entire application.
              Changes take effect immediately.
            </p>
            <div className="flex items-center justify-between bg-slate-50 rounded-xl border border-slate-200 px-4 py-3">
              <div className="flex items-center gap-3">
                <div className={`p-2 rounded-lg ${apisEnabled ? 'bg-violet-100' : 'bg-slate-100'}`}>
                  <ScanLine size={16} className={apisEnabled ? 'text-violet-600' : 'text-slate-400'} />
                </div>
                <div>
                  <p className="text-sm font-semibold text-slate-700">COPS ↔ APIS</p>
                  <p className="text-xs text-slate-500">Passenger intelligence — match APIS Excel against COPS database</p>
                </div>
              </div>
              <button type="button" disabled={flagsLoading} onClick={() => toggleApis(!apisEnabled)}
                className={`flex items-center gap-2 px-4 py-1.5 rounded-lg text-sm font-semibold transition-colors disabled:opacity-60 ${
                  apisEnabled ? 'bg-violet-600 hover:bg-violet-700 text-white' : 'bg-slate-200 hover:bg-slate-300 text-slate-700'
                }`}>
                {apisEnabled ? <><ToggleRight size={16} /> Enabled</> : <><ToggleLeft size={16} /> Disabled</>}
              </button>
            </div>
            {flagsMsg && <p className={`text-xs ${flagsMsg.includes('Failed') ? 'text-red-600' : 'text-emerald-700'}`}>{flagsMsg}</p>}
          </section>
        </div>
      )}

      {/* ══ TAB: BACKUP & RESTORE ════════════════════════════════════════════════ */}
      {activeTab === 'backup' && (
        <div className="space-y-5">

          {/* ── Compact backup — the one officers should use ── */}
          <section className="bg-white rounded-xl border border-emerald-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <Database size={18} className="text-emerald-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">
                  Backup
                  <span className="ml-1 text-xs font-normal text-emerald-700 bg-emerald-50 border border-emerald-200 rounded px-1.5 py-0.5">Recommended</span>
                </h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Every record — OS cases, BR, detention, plus users, print templates, duty
                  rates and all settings. Compressed and password-protected, so it is about
                  six times smaller than a copy of the database and does not slow down as the
                  data grows.
                </p>
              </div>
            </div>
            <div className="flex items-center gap-3">
              <button onClick={saveCompactBackup} disabled={compactLoading}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-60">
                <Download size={13} />
                {compactLoading ? 'Working…' : 'Save Backup'}
              </button>
              {autoStatus?.last_success && (
                <span className="text-xs text-slate-500">
                  Automatic backup last succeeded {new Date(autoStatus.last_success).toLocaleString()}
                </span>
              )}
            </div>
            {compactMsg && (
              <p className={`text-xs ${compactErr ? 'text-red-600' : 'text-emerald-700'}`}>{compactMsg}</p>
            )}
          </section>

          {/* ── The copy that leaves the building ── */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <CloudUpload size={18} className="text-blue-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Monthly copy to WorkDrive</h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Sent on its own, so nobody has to carry one out on a pen drive.
                </p>
              </div>
              {cloud && (
                <span className={`ml-auto text-xs font-bold px-2 py-0.5 rounded-full ${
                  cloud.enabled && cloud.configured && !cloud.needs_manual_backup
                    ? 'bg-emerald-100 text-emerald-700'
                    : 'bg-amber-100 text-amber-700'}`}>
                  {!cloud.configured ? 'NOT SET UP'
                    : !cloud.enabled ? 'OFF'
                    : cloud.needs_manual_backup ? 'NOT WORKING' : 'WORKING'}
                </span>
              )}
            </div>

            {cloud && (
              <div className="text-xs text-slate-600 space-y-1">
                <p>
                  {cloud.last_success
                    ? `Last sent ${new Date(cloud.last_success).toLocaleString()}`
                    : 'Nothing has been sent yet.'}
                  {cloud.days_since_success !== null && cloud.days_since_success !== undefined
                    ? ` — ${cloud.days_since_success} day(s) ago.` : ''}
                </p>
                {cloud.last_error && (
                  <p className="text-red-600 break-words">{cloud.last_error}</p>
                )}
                {!cloud.credential_store && (
                  <p className="text-amber-700 bg-amber-50 border border-amber-200 rounded-lg px-3 py-2">
                    This machine has no credential store, so there is nowhere safe to keep the
                    token. It is not written anywhere else — set this up on a machine that has one.
                  </p>
                )}
                <p className="text-slate-500">
                  While this is working the monthly reminder stays away. It returns if the copy
                  stops going out — that is the moment a pen drive is the only copy that would
                  survive.
                </p>
              </div>
            )}

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
              {([
                ['client_id',     'Client ID'],
                ['client_secret', 'Client secret'],
                ['folder_id',     'WorkDrive folder ID'],
                ['refresh_token', 'Refresh token (stored by Windows, not by COPS)'],
              ] as const).map(([key, label]) => (
                <label key={key} className="block">
                  <span className="text-[11px] uppercase tracking-wide text-slate-500">{label}</span>
                  <input
                    type={key === 'client_secret' || key === 'refresh_token' ? 'password' : 'text'}
                    autoComplete="off"
                    value={cloudForm[key]}
                    onChange={e => setCloudForm(f => ({ ...f, [key]: e.target.value }))}
                    className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg
                               focus:outline-none focus:ring-2 focus:ring-blue-400"
                  />
                </label>
              ))}
              <label className="block">
                <span className="text-[11px] uppercase tracking-wide text-slate-500">Sign-in host</span>
                <input
                  value={cloudForm.accounts_base}
                  onChange={e => setCloudForm(f => ({ ...f, accounts_base: e.target.value }))}
                  className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg font-mono"
                />
              </label>
              <label className="block">
                <span className="text-[11px] uppercase tracking-wide text-slate-500">API host</span>
                <input
                  value={cloudForm.api_base}
                  onChange={e => setCloudForm(f => ({ ...f, api_base: e.target.value }))}
                  className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg font-mono"
                />
              </label>
              <label className="block">
                <span className="text-[11px] uppercase tracking-wide text-slate-500">Send every (days)</span>
                <input
                  type="number" min={1} max={365}
                  value={cloudForm.every_days}
                  onChange={e => setCloudForm(f => ({ ...f, every_days: e.target.value }))}
                  className="mt-1 w-full px-2 py-1.5 text-sm border border-slate-300 rounded-lg"
                />
              </label>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                disabled={cloudBusy}
                onClick={() => saveCloud(true)}
                className="px-4 py-2 text-xs font-semibold rounded-lg bg-blue-600 text-white
                           hover:bg-blue-700 disabled:opacity-60"
              >
                {cloudBusy ? 'Working…' : 'Save and turn on'}
              </button>
              <button
                type="button"
                disabled={cloudBusy || !cloud?.configured}
                onClick={runCloudNow}
                className="px-3 py-2 text-xs rounded-lg border border-slate-300 text-slate-700
                           hover:bg-slate-50 disabled:opacity-50"
                title="Send one now, to prove the arrangement works before trusting it"
              >
                Send one now
              </button>
              <button
                type="button"
                disabled={cloudBusy}
                onClick={() => saveCloud(false)}
                className="px-3 py-2 text-xs rounded-lg border border-slate-300 text-slate-600 hover:bg-slate-50"
              >
                Turn off
              </button>
              {cloudMsg && <span className="text-xs text-slate-600 break-words">{cloudMsg}</span>}
            </div>
          </section>

          {/* ── Automatic backups ── */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <RefreshCw size={18} className="text-slate-500" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Automatic Backups</h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  {autoStatus
                    ? `Every ${autoStatus.interval_minutes} minutes, keeping ${autoStatus.keep_copies} copies in each folder.`
                    : 'Checking…'}
                </p>
              </div>
            </div>

            {autoStatus && autoStatus.destinations.length === 0 && (
              <p className="text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded-lg px-3 py-2">
                No backup folder is set, so no automatic backups are being taken. Set one on
                another machine on the network — a backup that only ever lands on this
                computer does not survive this computer failing.
              </p>
            )}

            {autoStatus && autoStatus.destinations.length > 0 && (
              <>
                <ul className="space-y-1.5">
                  {autoStatus.destinations.map((d) => (
                    <li key={d.path} className="flex items-start gap-2 text-xs">
                      <span className={`mt-0.5 w-2 h-2 rounded-full shrink-0 ${d.reachable ? 'bg-emerald-500' : 'bg-red-500'}`} />
                      <span className="font-mono text-slate-700 break-all">{d.path}</span>
                      <span className={d.reachable ? 'text-slate-500' : 'text-red-600'}>
                        {d.reachable ? (d.off_machine ? 'another machine' : 'this machine') : d.detail}
                      </span>
                    </li>
                  ))}
                </ul>
                {!autoStatus.any_off_machine && (
                  <p className="text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded-lg px-3 py-2">
                    Every backup folder is on this computer. If this machine fails, the
                    backups fail with it. Add a folder on another PC on the network.
                  </p>
                )}
              </>
            )}

            {autoStatus && autoStatus.destinations.length > 0 && !autoStatus.healthy && (
              <p className="text-xs text-red-700 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
                <strong>No backup has succeeded in the last 24 hours.</strong>{' '}
                {autoStatus.last_success
                  ? `The last one was ${new Date(autoStatus.last_success).toLocaleString()}.`
                  : 'None has ever succeeded on this machine.'}{' '}
                Check that the folders above are reachable — a backup that stops
                working quietly is only discovered on the day it is needed.
              </p>
            )}

            {autoStatus && autoStatus.composition?.length > 0 && (
              <details className="text-xs">
                <summary className="cursor-pointer text-slate-600 hover:text-slate-800">
                  What is in the backup
                </summary>
                <ul className="mt-1.5 space-y-0.5 pl-3">
                  {autoStatus.composition.map(c => (
                    <li key={c.table} className="flex justify-between max-w-sm text-slate-600">
                      <span className="font-mono">{c.table}</span>
                      <span className="tabular-nums">{c.rows.toLocaleString()} rows</span>
                    </li>
                  ))}
                </ul>
                <p className="mt-1.5 text-slate-500 max-w-lg">
                  Shown so you can see whether any one table is starting to dominate.
                  If it is, that table can be left out of the backup — but only
                  where a copy exists elsewhere, as the duty reports do once
                  they are emailed. Case records are never left out.
                </p>
                {autoStatus.excluded_tables && (
                  <p className="mt-1 text-amber-700">
                    Currently excluded: <span className="font-mono">{autoStatus.excluded_tables}</span>
                  </p>
                )}
              </details>
            )}

            {autoStatus?.refusing && (
              <p className="text-xs text-red-700 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
                <strong>Automatic backups are being refused.</strong> {autoStatus.refusing} —
                the existing backups have been left untouched deliberately. Check the
                database before overriding this.
              </p>
            )}
          </section>

          {/* ── The one backup ── */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <Download size={18} className="text-blue-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Download backup</h2>
                <p className="text-xs text-slate-500 mt-0.5 break-words">
                  One file with <strong>everything</strong> — every case, receipt, user and setting
                  from the first record to today. Compressed to about a sixth of the database, and
                  encrypted: it opens only with this office's own key, so it is safe to carry on a
                  drive. The newest file replaces the last; there is no need to keep the old ones.
                </p>
              </div>
            </div>
            <div className="flex items-center gap-3">
              <button onClick={downloadBackup} disabled={backupLoading}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60">
                <Download size={13} />
                {backupLoading ? (backupProgress || 'Preparing…') : 'Download backup'}
              </button>
            </div>
            {backupMsg && <p className={`text-xs ${backupMsg.includes('failed') ? 'text-red-600' : 'text-emerald-700'}`}>{backupMsg}</p>}
          </section>

          {/* ── Restore ── */}
          <section className="bg-white rounded-xl border border-red-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <Upload size={18} className="text-red-500" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Restore backup</h2>
                <p className="text-xs text-slate-500 mt-0.5 break-words">
                  Choose a backup file and it is put back in full. The current backup is a{' '}
                  <code className="bg-slate-100 px-1 rounded">.cops</code> file; an older{' '}
                  <code className="bg-slate-100 px-1 rounded">.db</code> or{' '}
                  <code className="bg-slate-100 px-1 rounded">.zip</code> the office may still have is
                  taken too. <strong className="text-red-600">Replaces all current data</strong> — use
                  only to recover, and only with the right key.
                </p>
              </div>
            </div>
            <form onSubmit={uploadRestore} className="space-y-3">
              <div className="flex items-center gap-3 flex-wrap">
                <input ref={restoreRef} type="file" accept=".cops,.db,.zip"
                  onChange={e => { setRestoreFile(e.target.files?.[0] || null); setRestoreResult(''); setRestoreErr(''); }}
                  className="text-xs text-slate-600 file:mr-3 file:py-1.5 file:px-3 file:rounded-lg file:border-0 file:text-xs file:bg-slate-100 file:text-slate-700 hover:file:bg-slate-200" />
                <button type="submit" disabled={!restoreFile || restoreLoading}
                  className="flex items-center gap-2 px-4 py-1.5 text-xs rounded-lg bg-red-600 text-white hover:bg-red-700 disabled:opacity-50">
                  <Upload size={13} />
                  {restoreLoading ? 'Restoring…' : 'Restore backup'}
                </button>
              </div>
              {restoreErr    && <p className="text-xs text-red-600">{restoreErr}</p>}
              {restoreResult && <p className="text-xs text-emerald-700 font-medium">{restoreResult}</p>}
            </form>
          </section>

          {/* ── Migration export (admin tool, not a backup) ── */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <FileDown size={18} className="text-slate-500" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Export cases &amp; settings (for migration)</h2>
                <p className="text-xs text-slate-500 mt-0.5 break-words">
                  A ZIP of CSV files — cases, settings, template history, statutes, users — for
                  moving or merging data between machines. This is <strong>not</strong> a backup:
                  for that, use <em>Download backup</em> above. Restoring a ZIP only fills in
                  missing rows and never overwrites what is here.
                </p>
              </div>
            </div>
            <button onClick={downloadCsvMigration} disabled={csvBusy}
              className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-slate-700 text-white hover:bg-slate-800 disabled:opacity-60">
              <FileDown size={13} /> {csvBusy ? 'Preparing…' : 'Export CSV (ZIP)'}
            </button>
            {csvMsg && <p className="text-xs text-slate-600">{csvMsg}</p>}
          </section>

          {/* Import from old MDB — two CSVs */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
            <div className="flex items-center gap-2">
              <FileUp size={18} className="text-slate-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Import from Old Database (.mdb)</h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Export both tables from MS Access as CSV. Upload <strong>cops_master first</strong>, then cops_items. Duplicates are always skipped.
                </p>
              </div>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="border border-blue-100 bg-blue-50 rounded-lg p-4 space-y-3">
                <p className="text-xs font-semibold text-blue-800">Step 1 — cops_master.csv</p>
                <form onSubmit={uploadLegacy} className="space-y-2">
                  <input ref={legacyRef} type="file" accept=".csv"
                    onChange={e => setLegacyFile(e.target.files?.[0] || null)}
                    className="w-full text-xs text-slate-600 file:mr-2 file:py-1 file:px-2 file:rounded file:border-0 file:text-xs file:bg-white file:text-slate-700 hover:file:bg-slate-50" />
                  <button type="submit" disabled={!legacyFile || legacyLoading}
                    className="w-full flex items-center justify-center gap-2 py-1.5 text-xs rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50">
                    <Upload size={12} /> {legacyLoading ? 'Importing…' : 'Upload cops_master CSV'}
                  </button>
                  {legacyErr    && <p className="text-xs text-red-600">{legacyErr}</p>}
                  {legacyResult && <p className="text-xs text-emerald-700 font-medium">{legacyResult}</p>}
                </form>
              </div>
              <div className="border border-amber-100 bg-amber-50 rounded-lg p-4 space-y-3">
                <p className="text-xs font-semibold text-amber-800">Step 2 — cops_items.csv</p>
                <form onSubmit={uploadLegacyItems} className="space-y-2">
                  <input ref={legacyItemsRef} type="file" accept=".csv"
                    onChange={e => setLegacyItemsFile(e.target.files?.[0] || null)}
                    className="w-full text-xs text-slate-600 file:mr-2 file:py-1 file:px-2 file:rounded file:border-0 file:text-xs file:bg-white file:text-slate-700 hover:file:bg-slate-50" />
                  <button type="submit" disabled={!legacyItemsFile || legacyItemsLoading}
                    className="w-full flex items-center justify-center gap-2 py-1.5 text-xs rounded-lg bg-amber-600 text-white hover:bg-amber-700 disabled:opacity-50">
                    <Upload size={12} /> {legacyItemsLoading ? 'Importing…' : 'Upload cops_items CSV'}
                  </button>
                  {legacyItemsErr    && <p className="text-xs text-red-600">{legacyItemsErr}</p>}
                  {legacyItemsResult && <p className="text-xs text-emerald-700 font-medium">{legacyItemsResult}</p>}
                </form>
              </div>
            </div>
          </section>

          {/* MDB direct import */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-3">
            <div className="flex items-center gap-2">
              <Database size={18} className="text-slate-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Import Directly from .mdb File</h2>
                <p className="text-xs text-slate-500 mt-0.5">Select the .mdb file from this computer. Both tables are imported in one pass — no conversion needed.</p>
              </div>
            </div>
            <form onSubmit={importMdb} className="space-y-3">
              <input type="file" accept=".mdb"
                onChange={e => { setMdbFile(e.target.files?.[0] || null); setMdbResult(''); setMdbErr(''); }}
                className="block w-full text-xs text-slate-600 file:mr-3 file:py-1.5 file:px-3 file:rounded file:border-0 file:text-xs file:bg-slate-100 file:text-slate-700 hover:file:bg-slate-200 cursor-pointer" />
              {mdbFile && (
                <p className="text-xs text-slate-500">Selected: <span className="font-mono">{mdbFile.name}</span> ({(mdbFile.size / 1024 / 1024).toFixed(1)} MB)</p>
              )}
              <div className="bg-amber-50 border border-amber-200 rounded p-2 text-xs text-amber-700">
                Reads both <strong>cops_master</strong> and <strong>cops_items</strong> in one pass. Large files may take several minutes.
              </div>
              <button type="submit" disabled={!mdbFile || mdbLoading}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-slate-800 text-white hover:bg-slate-700 disabled:opacity-50">
                <Database size={13} /> {mdbLoading ? 'Importing — please wait…' : 'Start MDB Import'}
              </button>
              {mdbErr    && <p className="text-xs text-red-600">{mdbErr}</p>}
              {mdbResult && <p className="text-xs text-emerald-700 font-medium">{mdbResult}</p>}
            </form>
          </section>

        </div>
      )}

      {/* ══ TAB: OS CONFIG ═══════════════════════════════════════════════════ */}
      {activeTab === 'osconfig' && (
        <div className="space-y-6">
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-4">
            <div className="flex items-center gap-2 mb-4">
              <Settings size={16} className="text-slate-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">OS Print Template Editor</h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Click any highlighted section in the OS layout below to edit it.
                  Changes are versioned — older OS cases always display the text that was in effect when they were created.
                </p>
              </div>
            </div>
            <OSTemplateEditor adminToken={adminToken} />
          </section>
        </div>
      )}

      {/* ══ TAB: STATUTES ════════════════════════════════════════════════════════ */}
      {activeTab === 'statutes' && (
        <StatutesAdmin adminToken={adminToken} />
      )}

      {/* ══ TAB: DANGER ZONE ═════════════════════════════════════════════════════ */}
      {activeTab === 'danger' && (
        <div className="space-y-5">

          {/* Warning banner */}
          <div className="bg-red-50 border border-red-300 rounded-xl p-4 flex items-start gap-3">
            <AlertTriangle size={20} className="text-red-600 mt-0.5 shrink-0" />
            <div>
              <p className="text-sm font-bold text-red-700">Irreversible Actions — Proceed with Extreme Caution</p>
              <p className="text-xs text-red-600 mt-1">
                Operations in this section permanently destroy data. There is no undo, no recycle bin, and no backup created automatically.
                Every record associated with the case — items, adjudication, warehouse, receipts, archive snapshots — will be wiped from the database.
              </p>
            </div>
          </div>

          {/* Permanent OS Case Deletion */}
          <section className="bg-white rounded-xl border-2 border-red-200 shadow-sm p-5">
            <div className="flex items-center gap-2 mb-1">
              <Trash2 size={16} className="text-red-600" />
              <h2 className="text-sm font-bold text-red-700">Permanent OS Case Deletion</h2>
            </div>
            <p className="text-xs text-slate-500 mb-4">
              Enter the OS No. and Year to permanently delete the case and all linked data:
              items, appeals, BR/DR receipts, warehouse entries, report staging, and all archive snapshots.
              The case will cease to exist in both the SDO module and the Adjudication module.
            </p>

            <form onSubmit={handlePurgeOS} className="space-y-4">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-semibold text-slate-600 mb-1">OS No.</label>
                  <input
                    type="text"
                    value={purgeOsNo}
                    onChange={e => { setPurgeOsNo(e.target.value); setPurgeResult(null); setPurgeErr(''); }}
                    placeholder="e.g. 252"
                    className="w-full border border-slate-300 rounded-md text-sm px-3 py-2 focus:ring-red-400 focus:border-red-400"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-600 mb-1">OS Year</label>
                  <input
                    type="number"
                    value={purgeYear}
                    onChange={e => { setPurgeYear(e.target.value); setPurgeResult(null); setPurgeErr(''); }}
                    placeholder="e.g. 2026"
                    className="w-full border border-slate-300 rounded-md text-sm px-3 py-2 focus:ring-red-400 focus:border-red-400"
                  />
                </div>
              </div>

              {/* Confirmation checkbox */}
              <label className="flex items-start gap-2 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={purgeConfirmed}
                  onChange={e => setPurgeConfirmed(e.target.checked)}
                  className="mt-0.5 accent-red-600"
                />
                <span className="text-xs text-slate-700">
                  I understand that this action is <strong>permanent and irreversible</strong>. All data for OS&nbsp;
                  <span className="font-mono font-bold">{purgeOsNo || '—'}/{purgeYear || '—'}</span> will be
                  deleted without any trace or archive.
                </span>
              </label>

              {/* Admin password */}
              <div>
                <label className="block text-xs font-semibold text-slate-600 mb-1">
                  Admin Password (re-enter to confirm)
                </label>
                <div className="relative">
                  <input
                    type={showPurgePwd ? 'text' : 'password'}
                    value={purgePwd}
                    onChange={e => setPurgePwd(e.target.value)}
                    placeholder="Admin password"
                    className="w-full border border-slate-300 rounded-md text-sm px-3 py-2 pr-10 focus:ring-red-400 focus:border-red-400"
                  />
                  <button type="button" onClick={() => setShowPurgePwd(p => !p)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600">
                    {showPurgePwd ? <EyeOff size={15} /> : <Eye size={15} />}
                  </button>
                </div>
              </div>

              {purgeErr && (
                <p className="text-xs text-red-600 font-medium flex items-center gap-1">
                  <AlertTriangle size={12} /> {purgeErr}
                </p>
              )}

              <button
                type="submit"
                disabled={purgeLoading || !purgeOsNo.trim() || !purgeYear.trim() || !purgePwd || !purgeConfirmed}
                className="flex items-center gap-2 px-5 py-2 text-sm font-semibold text-white bg-red-600 rounded-lg hover:bg-red-700 disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {purgeLoading ? <RefreshCw size={14} className="animate-spin" /> : <Trash2 size={14} />}
                {purgeLoading ? 'Deleting…' : 'Delete Permanently'}
              </button>
            </form>

            {/* Result */}
            {purgeResult && (
              <div className="mt-4 bg-green-50 border border-green-200 rounded-lg p-4 space-y-2">
                <p className="text-sm font-semibold text-green-800 flex items-center gap-1.5">
                  <ShieldCheck size={15} /> {purgeResult.message}
                </p>
                <p className="text-xs text-green-700">
                  Total rows removed: <strong>{purgeResult.total_rows_deleted}</strong>
                </p>
                {Object.keys(purgeResult.breakdown).length > 0 && (
                  <div className="text-xs text-green-700 space-y-0.5">
                    <p className="font-medium">Breakdown:</p>
                    {Object.entries(purgeResult.breakdown).map(([tbl, cnt]) => (
                      <div key={tbl} className="flex justify-between font-mono">
                        <span>{tbl}</span>
                        <span className="font-bold">{cnt}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </section>

          {/* ── Trial / Licence ─────────────────────────────────────────────
              The administrator sets the expiry outright: choose a length,
              restart the window, or disable it so the installation never
              expires. Same three controls as the Python app. */}
          <section className="bg-white rounded-xl border border-slate-200 shadow-sm p-5 space-y-4">
            <div className="flex items-center gap-2">
              <Clock size={18} className="text-blue-600" />
              <div>
                <h2 className="text-sm font-semibold text-slate-700">Trial / Licence</h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Set how long this installation runs before it expires, restart the
                  window, or make it permanent. Only an administrator can change this.
                </p>
              </div>
            </div>

            {trialStatus ? (
              trialStatus.trial_disabled ? (
                <div className="flex items-center gap-2 px-4 py-3 bg-green-50 border border-green-200 rounded-lg">
                  <CheckCircle size={16} className="text-green-600 shrink-0" />
                  <p className="text-sm font-medium text-green-800">
                    Permanent installation — this copy does not expire.
                  </p>
                </div>
              ) : (
                <div className={`flex items-center gap-3 px-4 py-3 rounded-lg border ${
                  trialStatus.expired ? 'bg-red-50 border-red-300' :
                  (trialStatus.days_remaining ?? trialStatus.trial_days) <= 7 ? 'bg-orange-50 border-orange-300' :
                  'bg-blue-50 border-blue-200'
                }`}>
                  <Clock size={16} className={trialStatus.expired ? 'text-red-600' : 'text-blue-600'} />
                  <p className={`text-sm font-semibold ${trialStatus.expired ? 'text-red-800' : 'text-blue-800'}`}>
                    {trialStatus.expired
                      ? `Expired — the ${trialStatus.trial_days}-day window has ended`
                      : `${trialStatus.days_remaining} of ${trialStatus.trial_days} day${trialStatus.trial_days === 1 ? '' : 's'} remaining`}
                  </p>
                </div>
              )
            ) : (
              <p className="text-xs text-slate-400 italic">Loading licence status…</p>
            )}

            <div className="border border-slate-200 rounded-lg p-3 bg-slate-50/50 space-y-2">
              <label className="block text-[11px] font-semibold text-slate-600">
                Duration in days
                <span className="ml-1 font-normal text-slate-400">— between 1 and 3650</span>
              </label>
              <div className="flex items-center gap-2 flex-wrap">
                <input
                  type="number" min={1} max={3650}
                  value={trialDaysInput}
                  onChange={e => setTrialDaysInput(e.target.value)}
                  className="w-24 border border-slate-200 rounded-md px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-blue-400"
                />
                <button type="button" disabled={trialLoading} onClick={handleTrialDaysSave}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-slate-700 text-white hover:bg-slate-800 disabled:opacity-60 font-semibold">
                  Save duration
                </button>
                <span className="text-[10.5px] text-slate-500">
                  Applies to the current window; use Reset to start it from today.
                </span>
              </div>
            </div>

            <div className="flex gap-3 flex-wrap">
              <button type="button" disabled={trialLoading} onClick={handleTrialReset}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60 font-semibold">
                <RefreshCw size={13} /> Reset — restart the {trialStatus?.trial_days ?? 30}-day window today
              </button>
              <button type="button" disabled={trialLoading || !!trialStatus?.trial_disabled}
                onClick={handleTrialDisable}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-green-700 text-white hover:bg-green-800 disabled:opacity-60 font-semibold">
                <CheckCircle size={13} /> Make permanent
              </button>
            </div>
            {trialMsg && (
              <p className={`text-xs font-medium ${trialMsg.startsWith('Failed') || trialMsg.startsWith('Enter') ? 'text-red-600' : 'text-emerald-700'}`}>
                {trialMsg}
              </p>
            )}
          </section>
        </div>
      )}

    </div>
    </div>
  );
}
