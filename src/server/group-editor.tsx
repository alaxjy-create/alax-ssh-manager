import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { saveGroup } from "@/lib/tauri";
import { useAppStore } from "@/stores/app-store";
import type { GroupFormInput, ServerGroup } from "@/types/app";

interface GroupEditorProps {
  open: boolean;
  group?: ServerGroup | null;
  onClose: () => void;
}

export function GroupEditor({ open, group, onClose }: GroupEditorProps) {
  const groups = useAppStore((state) => state.groups);
  const upsertGroup = useAppStore((state) => state.upsertGroup);
  const initial = useMemo<GroupFormInput>(
    () => ({
      id: group?.id,
      name: group?.name ?? "",
      parentId: group?.parentId ?? null,
      sortOrder: group?.sortOrder ?? groups.length + 1,
    }),
    [group, groups.length],
  );
  const [form, setForm] = useState(initial);

  if (!open) {
    return null;
  }

  async function submit() {
    if (!form.name.trim()) {
      return;
    }

    const saved = await saveGroup(form);
    upsertGroup(form, saved);
    onClose();
  }

  return (
    <Dialog
      open={open}
      title={group ? "编辑分组" : "新增分组"}
      onClose={onClose}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            取消
          </Button>
          <Button onClick={submit}>保存</Button>
        </>
      }
    >
      <div className="grid gap-4 md:grid-cols-2">
        <label className="space-y-1.5">
          <span className="text-xs font-medium text-muted-foreground">分组名称</span>
          <Input value={form.name} onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))} />
        </label>
        <label className="space-y-1.5">
          <span className="text-xs font-medium text-muted-foreground">排序</span>
          <Input type="number" value={form.sortOrder} onChange={(event) => setForm((current) => ({ ...current, sortOrder: Number(event.target.value) }))} />
        </label>
      </div>
    </Dialog>
  );
}
