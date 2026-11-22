// Phosphor: regular for chrome, bold beside text, duotone for the summary tiles. Strokes land on whole pixels at 16, 20 and 24px, which keeps them crisp; the light weight was rendering as a haze at these sizes. Names mirror the old Carbon set so
// call sites did not need to change.
import {
  ArrowDownRight,
  ArrowUpRight,
  Bell,
  BellSlash,
  BookOpenText,
  CheckCircle,
  CircleDashed,
  CircleHalf,
  Clock,
  Gauge,
  Hourglass,
  ListChecks,
  Prohibit,
  Receipt,
  ShieldWarning,
  Stack,
  Target,
  TrendUp,
  XCircle,
  Crosshair,
  MagnifyingGlass,
  Moon,
  Plus,
  Rows,
  SignOut,
  Sliders,
  SquaresFour,
  Sun,
  Trash,
  Warning,
  WarningCircle,
  WifiHigh,
  X,
  type Icon,
  type IconProps,
} from '@phosphor-icons/react';

const light = (I: Icon) => (p: IconProps) => <I weight="regular" {...p} />;
const fill = (I: Icon) => (p: IconProps) => <I weight="fill" {...p} />;
// Chip icons sit next to 600-weight text, so they use the bold weight: a 1.5px stroke at 16px.
const duo = (I: Icon) => (p: IconProps) => <I weight="duotone" {...p} />;
const bold = (I: Icon) => (p: IconProps) => <I weight="bold" {...p} />;

export const Close = bold(X);
export const Search = bold(MagnifyingGlass);
export const Asleep = bold(Moon);
export const Light = bold(Sun);
export const Logout = bold(SignOut);
export const Notification = bold(Bell);
export const NotificationOff = bold(BellSlash);
export const Dashboard = light(SquaresFour);
export const Table = light(Rows);
export const Book = light(BookOpenText);
export const WarningAlt = light(Warning);
export const ConnectionSignal = light(WifiHigh);
export const Settings = light(Sliders);
export const Security = bold(Crosshair);
export const Add = light(Plus);
export const TrashCan = light(Trash);
export const CheckmarkFilled = fill(CheckCircle);
export const ErrorFilled = fill(WarningCircle);
export const WarningAltFilled = fill(Warning);
export const CircleDash = bold(CircleDashed);

// Order states, sides and summary figures.
export const StateOpen = bold(CircleDashed);
export const StatePending = bold(Hourglass);
export const StateTrigger = bold(Target);
export const StatePart = bold(CircleHalf);
export const StateDone = fill(CheckCircle);
export const StateCancelled = bold(XCircle);
export const StateRejected = bold(Prohibit);
export const StateAmo = bold(Clock);
export const SideBuy = bold(ArrowUpRight);
export const SideSell = bold(ArrowDownRight);
export const StatPositions = duo(Stack);
export const StatUnprotected = duo(ShieldWarning);
export const StatOrders = duo(ListChecks);
export const StatTrades = duo(Receipt);
export const StatPnl = duo(TrendUp);
export const StatExposure = duo(Gauge);
