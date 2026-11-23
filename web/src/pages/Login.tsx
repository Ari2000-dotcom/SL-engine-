import { useState } from 'react';
import { login } from '../api';
import { DeskBrand } from '../components/Shell';
import { View, ViewOff } from '../icons';
import type { Session } from '../types';

export function Login({ onSession }: { onSession: (s: Session) => void }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onSession(await login(username.trim(), password));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not sign in');
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="login">
      <section className="login__panel" aria-labelledby="login-title">
        <DeskBrand />
        <form onSubmit={submit}>
          <div>
            <h1 id="login-title">Sign in to RMS Desk</h1>
            <p>Use your desk credentials to access the monitor.</p>
          </div>
          {error && <div className="banner" role="alert">{error}</div>}
          <label className="fld">
            <span>Username</span>
            <input name="username" autoComplete="username" autoFocus value={username} onChange={(e) => setUsername(e.target.value)} />
          </label>
          <div className="fld">
            <label htmlFor="password">Password</label>
            <div className="password-field">
              <input id="password" name="password" type={showPassword ? 'text' : 'password'} autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
              <button type="button" onClick={() => setShowPassword((show) => !show)} aria-label={showPassword ? 'Hide password' : 'Show password'}>
                {showPassword ? <ViewOff size={16} aria-hidden="true" /> : <View size={16} aria-hidden="true" />}
              </button>
            </div>
          </div>
          <button type="submit" className="btn btn--primary login__submit" disabled={busy || !username.trim() || !password}>
            {busy ? 'Signing in…' : 'Sign in'}
          </button>
        </form>
      </section>
    </main>
  );
}
