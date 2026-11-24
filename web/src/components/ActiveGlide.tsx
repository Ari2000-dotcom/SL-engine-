import { motion, useReducedMotion } from 'motion/react';
import { motionTokens } from './arc/lib/motion-tokens';

/**
 * The highlight behind the active tab. Render it inside the active button only. Every button in one group passes the
 * same `group` id, so motion moves this single highlight from the old tab to the new one instead of swapping it in place.
 * The button needs `position: relative`; its other children sit above the highlight (see `.glide` in styles.scss).
 */
export function ActiveGlide({ group, variant = 'pill' }: { group: string; variant?: 'pill' | 'line' | 'nav' }) {
  const reduce = useReducedMotion();
  return (
    <motion.span
      layoutId={group}
      aria-hidden="true"
      className={`glide glide--${variant}`}
      transition={reduce ? { duration: 0 } : motionTokens.spring.morph}
    />
  );
}
