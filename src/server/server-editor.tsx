import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { saveServer } from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";
import type { AuthType, ServerFormInput, ServerProfile } from "@/types/app";

interface ServerEditorProps {
  open: boolean;
  server?: ServerProfile | null;
  onClose: () => void;
}

const emptyServer: ServerFormInput = {
  name: "",
  host: "",
  port: 22,
  username: "",
  authType: "password",
  groupId: null,
  tags: [],
  note: "",
};

export function ServerEditor({ open, server, onClose }: ServerEditorProps) {
  const groups = useAppStore((state) => state.groups);
  const upsertServer = useAppStore((state) => state.upsertServer);
  const addLog = useAppStore((state) => state.addLog);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const initial = useMemo<ServerFormInput>(() => {
    if (!server) {
      return emptyServer;
    }

    return {
      id: server.id,
      name: server.name,
      host: server.host,
      port: server.port,
      username: server.username,
      authType: server.authType,
      groupId: server.groupId,
      tags: server.tags,
      note: server.note,
    };
  }, [server]);

  const [form, setForm] = useState(initial);

  useEffect(() => {
    if (open) {
      setForm(initial);
      setErrorMessage(null);
      setSaving(false);
    }
  }, [initial, open]);

  if (!open) {
    return null;
  }

  function update<K extends keyof ServerFormInput>(key: K, value: ServerFormInput[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function submit() {
    const input = {
      ...form,
      name: form.name.trim(),
      host: form.host.trim(),
      username: form.username.trim(),
      tags: form.tags.map((tag) => tag.trim()).filter(Boolean),
    };

    if (!input.name || !input.host || !input.username) {
      const message = "服务器名称、Host、用户名不能为空";
      setErrorMessage(message);
      addLog({ level: "warn", category: "server", message });
      return;
    }

    setSaving(true);
    setErrorMessage(null);
    try {
      const saved = await saveServer(input);
      upsertServer(input, saved);
      onClose();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setErrorMessage(message);
      addLog({ level: "error", category: "server", message });
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog
      open={open}
      title={server ? "编辑服务器" : "新增服务器"}
      description="密码、私钥内容和 passphrase 只会提交给后端凭据存储，不写入 SQLite。"
      onClose={onClose}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            取消
          </Button>
          <Button onClick={submit} disabled={saving}>
            {saving ? "保存中" : "保存"}
          </Button>
        </>
      }
    >
      {errorMessage ? (
        <div className="mb-4 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {errorMessage}
        </div>
      ) : null}
      <div className="grid gap-4 md:grid-cols-2">
        <Field label="服务器名称">
          <Input value={form.name} onChange={(event) => update("name", event.target.value)} placeholder="例如：生产机" />
        </Field>
        <Field label="Host">
          <Input value={form.host} onChange={(event) => update("host", event.target.value)} placeholder="主机名或 IP 地址" />
        </Field>
        <Field label="端口">
          <Input type="number" min={1} max={65535} value={form.port} onChange={(event) => update("port", Number(event.target.value))} />
        </Field>
        <Field label="用户名">
          <Input value={form.username} onChange={(event) => update("username", event.target.value)} placeholder="如 root" />
        </Field>
        <Field label="认证方式">
          <select
            className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none"
            value={form.authType}
            onChange={(event) => update("authType", event.target.value as AuthType)}
          >
            <option value="password">密码登录</option>
            <option value="private_key">私钥登录</option>
            <option value="private_key_with_passphrase">私钥 + passphrase</option>
          </select>
        </Field>
        <Field label="所属分组">
          <select
            className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none"
            value={form.groupId ?? ""}
            onChange={(event) => update("groupId", event.target.value || null)}
          >
            <option value="">未分组</option>
            {groups.map((group) => (
              <option key={group.id} value={group.id}>
                {group.name}
              </option>
            ))}
          </select>
        </Field>
        {form.authType === "password" ? (
          <Field label="密码">
            <Input type="password" value={form.password ?? ""} onChange={(event) => update("password", event.target.value)} placeholder="留空则不修改已有密码" />
          </Field>
        ) : (
          <>
            <Field label="私钥路径">
              <Input value={form.privateKeyPath ?? ""} onChange={(event) => update("privateKeyPath", event.target.value)} placeholder="C:\\Users\\..." />
            </Field>
            <Field label="私钥内容">
              <Input value={form.privateKeyContent ?? ""} onChange={(event) => update("privateKeyContent", event.target.value)} placeholder="可选，优先使用路径" />
            </Field>
          </>
        )}
        {form.authType === "private_key_with_passphrase" ? (
          <Field label="Passphrase">
            <Input type="password" value={form.passphrase ?? ""} onChange={(event) => update("passphrase", event.target.value)} placeholder="留空则不修改" />
          </Field>
        ) : null}
        <Field label="标签">
          <Input value={form.tags.join(", ")} onChange={(event) => update("tags", event.target.value.split(","))} placeholder="Docker, NAS, 生产" />
        </Field>
        <Field label="备注">
          <Input value={form.note} onChange={(event) => update("note", event.target.value)} placeholder="用途、维护说明" />
        </Field>
      </div>
    </Dialog>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="space-y-1.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}
