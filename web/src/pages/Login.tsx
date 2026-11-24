import { useState } from 'react';
import { login } from '../api';
import { DeskBrand } from '../components/Shell';
import Alert from '../components/arc/alert/alert';
import { Input } from '../components/arc/input/input';
import { PasswordField } from '../components/arc/password-field/password-field';
import type { Session } from '../types';

export function Login({ onSession }: { onSession: (s: Session) => void }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
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
          {error && <Alert tone="danger" title={error} />}
          <Input label="Username" name="username" autoComplete="username" autoFocus value={username} onChange={(e) => setUsername(e.target.value)} />
          <PasswordField label="Password" name="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
          <button type="submit" className="btn btn--primary login__submit" disabled={busy || !username.trim() || !password}>
            {busy ? 'Signing in…' : 'Sign in'}
          </button>
        </form>
      </section>
    </main>
  );
}
