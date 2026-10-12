/**
 * Recover owner access: stub. Heading and subtitle only; the passkey flow is built separately.
 */
import {AuthLayout} from '../auth/AuthLayout';

export default function RecoverPage() {
  return <AuthLayout title="Recover owner access" subtitle="Create a new passkey for the owner account." />;
}
