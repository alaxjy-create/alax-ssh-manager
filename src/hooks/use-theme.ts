import { useAppStore } from "@/stores/app-store";

export function useTheme() {
  const theme = useAppStore((state) => state.theme);
  const setTheme = useAppStore((state) => state.setTheme);

  return { theme, setTheme };
}
