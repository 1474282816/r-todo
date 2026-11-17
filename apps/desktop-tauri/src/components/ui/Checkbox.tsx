import { motion } from "framer-motion";
import { Check } from "lucide-react";
import { cn } from "../../lib/cn";

interface Props {
  checked: boolean;
  onChange: () => void;
  size?: number;
}

/** Things 3 风格的空心圆勾选框 */
export function Checkbox({ checked, onChange, size = 20 }: Props) {
  return (
    <motion.button
      type="button"
      onClick={onChange}
      whileTap={{ scale: 0.85 }}
      className={cn(
        "flex shrink-0 items-center justify-center rounded-full border-2 transition-colors",
        checked
          ? "border-emerald-500 bg-emerald-500 text-white"
          : "border-neutral-300 bg-transparent hover:border-neutral-400 dark:border-neutral-500 dark:hover:border-neutral-300",
      )}
      style={{ width: size, height: size }}
      aria-checked={checked}
      role="checkbox"
    >
      {checked && (
        <motion.span
          initial={{ scale: 0, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ duration: 0.15 }}
        >
          <Check size={size * 0.65} strokeWidth={3} />
        </motion.span>
      )}
    </motion.button>
  );
}
