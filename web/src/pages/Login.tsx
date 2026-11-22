import { Button, InlineNotification, PasswordInput, TextInput } from '@carbon/react';
import { useState } from 'react';
import { login } from '../api';
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
    <div className="login">
      <section className="login__brand">
        <div>
          <h1>SL Guardian</h1>
          <p>Watches every client leg across Kotak Neo, Zerodha Kite and Angel One, and tells the desk the moment one is missing its stop-loss.</p>
        </div>
        <ul>
          <li>Reads order books, trade books and positions. It never places, changes or cancels an order.</li>
          <li>Checks for a missing, partial, rejected, cancelled, wrong-side or too-distant stop-loss on every leg.</li>
          <li>Refreshes on every broker order update and re-reads all books every 5 seconds.</li>
        </ul>
      </section>
      <section className="login__form">
        <form onSubmit={submit}>
          <div>
            <h2>Sign in</h2>
            <p className="login__hint">Use the username and password the desk administrator gave you.</p>
          </div>
          {error && (
            <InlineNotification kind="error" title="Sign-in failed" subtitle={error} hideCloseButton lowContrast />
          )}
          <TextInput
            id="username"
            size="md"
            labelText="Username"
            autoComplete="username"
            autoFocus
            value={username}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setUsername(e.target.value)}
          />
          <PasswordInput
            id="password"
            size="md"
            labelText="Password"
            autoComplete="current-password"
            value={password}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPassword(e.target.value)}
          />
          <Button type="submit" size="md" className="login__submit" disabled={busy || !username || !password}>
            {busy ? 'Signing in' : 'Sign in'}
          </Button>
        </form>
      </section>
    </div>
  );
}
