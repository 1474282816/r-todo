import { motion } from "framer-motion";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
}

export function IconButton({ children, className, ...rest }: Props) {
  return (
    <motion.button
      whileTap={{ scale: 0.9 }}
      whileHover={{ scale: 1.05 }}
      className={cn(
        "inline-flex h-8 w-8 items-center justify-center rounded-md text-neutral-500 transition-colors hover:bg-black/5 hover:text-neutral-900 dark:text-neutral-400 dark:hover:bg-white/10 dark:hover:text-neutral-100",
        className,
      )}
      {...(rest as object)}
    >
      {children}
    </motion.button>
  );
}
