import { Loader2 } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type CookieFormat, type Profile } from "../lib/api";

interface CookieDialogProps {
  /** "export" supports one or many profiles (bulk); "import" targets one. */
  mode: "export" | "import";
  profiles: Profile[];
  onClose: () => void;
  /** Toast message shown by the parent after the dialog closes. */
  onDone: (message: string) => void;
}

/** Trigger a browser download of `content` as `filename` (W19a pattern). */
function download(content: string, filename: string, mime: string) {
  const url = URL.createObjectURL(new Blob([content], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

const safeName = (name: string) => name.replace(/[\\/:*?"<>|]+/g, "_");

/**
 * Cookie import/export dialog (W24a) — CDP-backed, format JSON or Netscape.
 * Import accepts a file or pasted text (auto-detected by the backend).
 */
export function CookieDialog({ mode, profiles, onClose, onDone }: CookieDialogProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<CookieFormat>("json");
  const [tab, setTab] = useState<"file" | "text">("file");
  const [data, setData] = useState("");
  const [fileName, setFileName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const single = profiles[0];
  const title =
    mode === "import"
      ? t("cookies.importTitle", { name: single?.name })
      : profiles.length === 1
        ? t("cookies.exportTitle", { name: single?.name })
        : t("cookies.exportTitleMany", { count: profiles.length });

  const handleExport = async () => {
    setBusy(true);
    setError(null);
    const ext = format === "json" ? "json" : "txt";
    const mime = format === "json" ? "application/json" : "text/plain";
    let ok = 0;
    let cookieCount = 0;
    let lastError = "";
    for (const p of profiles) {
      try {
        const res = await api.exportCookies(p.id, format);
        download(res.data, `${safeName(p.name)}.cookies.${ext}`, mime);
        ok++;
        cookieCount += res.count;
      } catch (err) {
        lastError = String(err);
      }
    }
    setBusy(false);
    if (ok === 0) {
      setError(t("cookies.exportFailed", { error: lastError }));
      return;
    }
    onDone(
      profiles.length === 1
        ? t("cookies.exportedOne", { count: cookieCount, name: single?.name })
        : t("cookies.exportedMany", { ok, total: profiles.length }),
    );
    onClose();
  };

  const handleImport = async () => {
    if (!single || !data.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const count = await api.importCookies(single.id, data);
      onDone(t("cookies.importSuccess", { count, name: single.name }));
      onClose();
    } catch (err) {
      setError(t("cookies.importFailed", { error: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const handleFile = async (file: File) => {
    try {
      setData(await file.text());
      setFileName(file.name);
      setError(null);
    } catch (err) {
      setError(t("cookies.importFailed", { error: String(err) }));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onClose();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">{title}</h2>
        <p className="mt-1.5 text-xs text-fg-muted">{t("cookies.headlessHint")}</p>

        {mode === "export" ? (
          <fieldset className="mt-4">
            <legend className="label">{t("cookies.formatLabel")}</legend>
            <div className="flex gap-2">
              {(["json", "netscape"] as const).map((f) => (
                <button
                  key={f}
                  type="button"
                  aria-pressed={format === f}
                  onClick={() => setFormat(f)}
                  className={`btn h-9 border ${
                    format === f
                      ? "border-accent bg-[#F0F6FF] text-accent dark:bg-accent/10"
                      : "border-border bg-surface-2 text-fg hover:bg-surface-3"
                  }`}
                >
                  {f === "json" ? t("cookies.formatJson") : t("cookies.formatNetscape")}
                </button>
              ))}
            </div>
          </fieldset>
        ) : null}

        {mode === "import" ? (
          <div className="mt-4">
            <div
              className="inline-flex rounded-md border border-border p-0.5"
              role="tablist"
              aria-label={t("cookies.sourceLabel")}
            >
              {(["file", "text"] as const).map((k) => (
                <button
                  key={k}
                  type="button"
                  role="tab"
                  aria-selected={tab === k}
                  onClick={() => setTab(k)}
                  className={`h-8 rounded px-3 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
                    tab === k
                      ? "bg-[#F0F6FF] text-accent dark:bg-accent/10"
                      : "text-fg-muted hover:text-fg"
                  }`}
                >
                  {k === "file" ? t("cookies.tabFile") : t("cookies.tabText")}
                </button>
              ))}
            </div>

            {tab === "file" ? (
              <div className="mt-3">
                <button
                  type="button"
                  onClick={() => fileRef.current?.click()}
                  className="input h-11 cursor-pointer text-left"
                >
                  <span className={fileName ? "text-fg" : "text-fg-muted/60"}>
                    {fileName ?? t("cookies.chooseFile")}
                  </span>
                </button>
                <input
                  ref={fileRef}
                  type="file"
                  accept=".json,.txt,application/json,text/plain"
                  className="hidden"
                  tabIndex={-1}
                  aria-label={t("cookies.chooseFile")}
                  onChange={(e) => {
                    const file = e.currentTarget.files?.[0];
                    e.currentTarget.value = "";
                    if (file) void handleFile(file);
                  }}
                />
                <p className="mt-1.5 text-xs text-fg-muted">{t("cookies.fileHint")}</p>
              </div>
            ) : (
              <textarea
                value={data}
                onChange={(e) => {
                  setData(e.target.value);
                  setFileName(null);
                }}
                rows={6}
                spellCheck={false}
                placeholder={t("cookies.textPlaceholder")}
                aria-label={t("cookies.textPlaceholder")}
                className="input mt-3 resize-y font-mono text-xs"
              />
            )}
          </div>
        ) : null}

        {error && <p className="mt-3 text-sm text-danger">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            className="btn-secondary h-9"
            disabled={busy}
            onClick={onClose}
          >
            {t("cookies.cancel")}
          </button>
          <button
            type="button"
            className="btn-primary h-9"
            disabled={busy || (mode === "import" && !data.trim())}
            onClick={() => void (mode === "export" ? handleExport() : handleImport())}
          >
            {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />}
            {mode === "export" ? t("cookies.exportButton") : t("cookies.importButton")}
          </button>
        </div>
      </div>
    </div>
  );
}
