import { type ReactNode, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  isTauri,
  onUserbotAuthState,
  openExternal,
  type UserbotStatus,
} from "../../lib/api";

const MY_TELEGRAM_URL = "https://my.telegram.org/apps";

/** Labeled input row shared by every auth step (36px input like the section). */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="text-sm font-medium text-fg">{label}</span>
      {children}
    </label>
  );
}

/**
 * (W55b-UI) Userbot (MTProto) login panel — drives the auth state machine:
 * no_credentials → api_id/api_hash → waiting_phone → waiting_code →
 * waiting_password → ready. State transitions arrive via the
 * `userbot-auth-state` event; api_hash is write-only (never read back) and
 * no secret is ever logged. When the state reaches "ready" the panel
 * activates the userbot transport (`cloud_set_transport`).
 */
export function UserbotPanel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<UserbotStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Force the credentials form even when credentials exist (stuck "disconnected").
  const [reenterCreds, setReenterCreds] = useState(false);
  const [apiId, setApiId] = useState("");
  const [apiHash, setApiHash] = useState("");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  // Last state we activated the transport for — avoids repeated set calls.
  const activatedRef = useRef(false);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    api
      .userbotGetStatus()
      .then(setStatus)
      .catch(() => {});
    void onUserbotAuthState((s) => {
      setStatus(s);
      setBusy(false);
      setReenterCreds(false);
    }).then((f) => {
      if (disposed) f();
      else unlisten = f;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // One-time OTP / 2FA inputs must not survive a state change.
  const state = status?.state;
  useEffect(() => {
    setCode("");
    setPassword("");
    if (state !== "ready") activatedRef.current = false;
  }, [state]);

  // Ready → make userbot the active transport (backend accepts only then).
  useEffect(() => {
    if (state !== "ready" || activatedRef.current) return;
    activatedRef.current = true;
    api.cloudSetTransport("userbot").catch((err) => {
      setError(err instanceof Error ? err.message : String(err));
    });
  }, [state]);

  /** Run one auth action with shared busy/error handling. */
  const run = (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    action().catch((err) => {
      setBusy(false);
      setError(err instanceof Error ? err.message : String(err));
    });
  };

  const handleSaveCreds = () =>
    run(async () => {
      await api.userbotSetCredentials(
        Number.parseInt(apiId, 10),
        apiHash.trim(),
      );
      setApiId("");
      setApiHash("");
      setStatus(await api.userbotGetStatus());
      setBusy(false);
      setReenterCreds(false);
    });

  const handleRetry = () =>
    run(async () => {
      setStatus(await api.userbotGetStatus());
      setBusy(false);
    });

  const handleLogout = () =>
    run(async () => {
      await api.userbotLogout();
    });

  const inputCls = "input mt-1 w-full py-1.5 text-sm";
  const btnCls =
    "btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50";
  const canSaveCreds =
    Number.isInteger(Number.parseInt(apiId, 10)) &&
    Number.parseInt(apiId, 10) > 0 &&
    apiHash.trim() !== "";

  if (!status) {
    return <p className="text-xs text-fg-muted">{t("userbot.loading")}</p>;
  }

  const showCredsForm = status.state === "no_credentials" || reenterCreds;

  const step = showCredsForm ? (
    <>
      <p className="text-xs text-fg-muted">
        {t("userbot.credsHint")}{" "}
        <button
          type="button"
          onClick={() => openExternal(MY_TELEGRAM_URL)}
          className="text-accent underline-offset-2 hover:underline"
        >
          my.telegram.org
        </button>
      </p>
      <Field label={t("userbot.apiId")}>
        <input
          type="text"
          inputMode="numeric"
          value={apiId}
          onChange={(e) => setApiId(e.target.value)}
          autoComplete="off"
          placeholder="1234567"
          className={inputCls}
        />
      </Field>
      <Field label={t("userbot.apiHash")}>
        <input
          type="password"
          value={apiHash}
          onChange={(e) => setApiHash(e.target.value)}
          autoComplete="off"
          placeholder={
            status.state === "no_credentials"
              ? "0123456789abcdef…"
              : t("telegram.tokenSavedPlaceholder")
          }
          className={inputCls}
        />
      </Field>
      <button
        type="button"
        disabled={!isTauri() || busy || !canSaveCreds}
        onClick={handleSaveCreds}
        className={btnCls}
      >
        {t("userbot.saveCreds")}
      </button>
    </>
  ) : status.state === "disconnected" ? (
    <>
      <p className="text-xs text-fg-muted">{t("userbot.connecting")}</p>
      <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={!isTauri() || busy}
          onClick={handleRetry}
          className={btnCls}
        >
          {t("userbot.retry")}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => setReenterCreds(true)}
          className="text-xs text-accent underline-offset-2 hover:underline"
        >
          {t("userbot.reenterCreds")}
        </button>
      </div>
    </>
  ) : status.state === "waiting_phone" ? (
    <>
      <Field label={t("userbot.phone")}>
        <input
          type="tel"
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
          autoComplete="off"
          placeholder="+84901234567"
          className={inputCls}
        />
      </Field>
      <button
        type="button"
        disabled={!isTauri() || busy || phone.trim() === ""}
        onClick={() => run(() => api.userbotSendPhone(phone.trim()))}
        className={btnCls}
      >
        {t("userbot.sendPhone")}
      </button>
    </>
  ) : status.state === "waiting_code" ? (
    <>
      <p className="text-xs text-fg-muted">
        {status.phoneHint
          ? t("userbot.codeSentTo", { phone: status.phoneHint })
          : t("userbot.codeSent")}
      </p>
      <Field label={t("userbot.code")}>
        <input
          type="text"
          inputMode="numeric"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          autoComplete="one-time-code"
          placeholder="12345"
          className={inputCls}
        />
      </Field>
      <button
        type="button"
        disabled={!isTauri() || busy || code.trim() === ""}
        onClick={() => run(() => api.userbotSubmitCode(code.trim()))}
        className={btnCls}
      >
        {t("userbot.submitCode")}
      </button>
    </>
  ) : status.state === "waiting_password" ? (
    <>
      <Field label={t("userbot.password")}>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="off"
          className={inputCls}
        />
      </Field>
      <button
        type="button"
        disabled={!isTauri() || busy || password === ""}
        onClick={() => run(() => api.userbotSubmitPassword(password))}
        className={btnCls}
      >
        {t("userbot.submitPassword")}
      </button>
    </>
  ) : (
    // ready
    <div className="flex items-center justify-between gap-4">
      <p className="text-sm text-fg">
        {t("userbot.readyAs", { username: status.username || "?" })}
      </p>
      <button
        type="button"
        disabled={!isTauri() || busy}
        onClick={handleLogout}
        className={btnCls}
      >
        {t("userbot.logout")}
      </button>
    </div>
  );

  return (
    <div className="space-y-3">
      {step}
      {busy && <p className="text-xs text-fg-muted">{t("userbot.working")}</p>}
      {error && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
