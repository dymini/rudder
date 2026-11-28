import { Ionicons } from "@expo/vector-icons";
import { Children, Fragment, type ReactNode } from "react";
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import { colors, hairline, monoFont, radius, spacing, tapHaptic, type Tone } from "../lib/theme";

/** Architectural Dymini Panel brand mark matching the web console's deep-pine emblem. */
export function DyminiMark({ size = 40 }: { size?: number }) {
  const fontSize = Math.round(size * 0.52);
  const br = Math.round(size * 0.24);
  return (
    <View
      style={{
        width: size,
        height: size,
        borderRadius: br,
        backgroundColor: "#1a3a2f",
        borderWidth: 1,
        borderColor: colors.accentBorder,
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <Text
        style={{
          color: "#f5f4f0",
          fontSize,
          fontWeight: "700",
          letterSpacing: -0.5,
          includeFontPadding: false,
        }}
      >
        D
      </Text>
    </View>
  );
}

export function Screen({
  children,
  refreshing,
  onRefresh,
  scroll = true,
}: {
  children: ReactNode;
  refreshing?: boolean;
  onRefresh?: () => void;
  scroll?: boolean;
}) {
  if (!scroll) {
    return <View style={styles.screen}>{children}</View>;
  }
  return (
    <ScrollView
      style={styles.screen}
      contentContainerStyle={styles.screenContent}
      showsVerticalScrollIndicator={false}
      refreshControl={
        onRefresh ? (
          <RefreshControl
            refreshing={refreshing ?? false}
            onRefresh={onRefresh}
            tintColor={colors.accent}
          />
        ) : undefined
      }
    >
      {children}
    </ScrollView>
  );
}

export function Card({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[styles.card, style]}>{children}</View>;
}

export function CardTitle({ children }: { children: ReactNode }) {
  return <Text style={styles.cardTitle}>{children}</Text>;
}

/** Apple HIG inset-grouped container with automatic hairline dividers between items. */
export function InsetGroup({
  children,
  style,
}: {
  children: ReactNode;
  style?: StyleProp<ViewStyle>;
}) {
  const items = Children.toArray(children).filter(Boolean);
  return (
    <View style={[styles.insetGroup, style]}>
      {items.map((child, idx) => (
        <Fragment key={idx}>
          {idx > 0 ? <View style={styles.insetDivider} /> : null}
          {child}
        </Fragment>
      ))}
    </View>
  );
}

/** Single precision row inside an InsetGroup. */
export function GroupedRow({
  icon,
  title,
  subtitle,
  right,
  onPress,
  tone,
}: {
  icon?: keyof typeof Ionicons.glyphMap;
  title: string;
  subtitle?: string;
  right?: ReactNode;
  onPress?: () => void;
  tone?: Tone;
}) {
  const iconColor = tone ? toneColors[tone] : colors.textDim;
  return (
    <Pressable
      onPress={
        onPress
          ? () => {
              tapHaptic("select");
              onPress();
            }
          : undefined
      }
      disabled={!onPress}
      style={({ pressed }) => [
        styles.groupedRow,
        pressed && onPress ? { backgroundColor: colors.cardSoft } : null,
      ]}
    >
      {icon ? (
        <View style={styles.groupedIconWrap}>
          <Ionicons name={icon} size={17} color={iconColor} />
        </View>
      ) : null}
      <View style={{ flex: 1, minWidth: 0 }}>
        <Text style={styles.listTitle} numberOfLines={1}>
          {title}
        </Text>
        {subtitle ? (
          <Text style={styles.listSubtitle} numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {right}
      {onPress ? (
        <Ionicons name="chevron-forward" size={16} color={colors.textFaint} style={{ marginLeft: 2 }} />
      ) : null}
    </Pressable>
  );
}

/** Hairline-divided horizontal KPI ledger (mobile counterpart of web's MetricStrip). */
export function MetricLedger({
  items,
}: {
  items: {
    label: string;
    value: string;
    suffix?: string;
    caption?: string;
    tone?: Tone;
    onPress?: () => void;
  }[];
}) {
  return (
    <View style={styles.ledger}>
      {items.map((item, idx) => {
        const valColor =
          item.tone === "danger"
            ? colors.danger
            : item.tone === "warn"
              ? colors.warn
              : colors.text;
        const capColor =
          item.tone === "danger"
            ? colors.danger
            : item.tone === "warn"
              ? colors.warn
              : item.tone === "ok"
                ? colors.accent
                : colors.textDim;
        const inner = (
          <>
            <Text style={styles.ledgerLabel} numberOfLines={1}>
              {item.label}
            </Text>
            <View style={styles.ledgerValueRow}>
              <Text style={[styles.ledgerValue, { color: valColor }]} numberOfLines={1}>
                {item.value}
              </Text>
              {item.suffix ? (
                <Text style={styles.ledgerSuffix} numberOfLines={1}>
                  {item.suffix}
                </Text>
              ) : null}
            </View>
            {item.caption ? (
              <Text style={[styles.ledgerCaption, { color: capColor }]} numberOfLines={1}>
                {item.caption}
              </Text>
            ) : null}
          </>
        );
        return (
          <Fragment key={item.label}>
            {idx > 0 ? <View style={styles.ledgerDivider} /> : null}
            {item.onPress ? (
              <Pressable
                onPress={() => {
                  tapHaptic("select");
                  item.onPress?.();
                }}
                style={({ pressed }) => [
                  styles.ledgerCell,
                  pressed && { backgroundColor: colors.cardSoft },
                ]}
              >
                {inner}
              </Pressable>
            ) : (
              <View style={styles.ledgerCell}>{inner}</View>
            )}
          </Fragment>
        );
      })}
    </View>
  );
}

/** Native-feeling iOS segmented control with haptic selection feedback. */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly { value: T; label: string }[];
  value: T;
  onChange: (next: T) => void;
}) {
  return (
    <View style={styles.segmented}>
      {options.map((option) => {
        const active = option.value === value;
        return (
          <Pressable
            key={option.value}
            onPress={() => {
              if (!active) {
                tapHaptic("select");
                onChange(option.value);
              }
            }}
            style={[styles.segmentItem, active && styles.segmentActive]}
          >
            <Text
              style={[styles.segmentLabel, active && styles.segmentLabelActive]}
              numberOfLines={1}
            >
              {option.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

/** Crisp 4px telemetry bar with threshold coloring. */
export function HairlineBar({
  value,
  warn = 75,
  danger = 90,
  color,
}: {
  value: number;
  warn?: number;
  danger?: number;
  color?: string;
}) {
  const pct = Math.max(0, Math.min(100, value));
  const fill =
    color ??
    (pct >= danger ? colors.danger : pct >= warn ? colors.warn : colors.accent);
  return (
    <View style={styles.barTrack}>
      <View style={[styles.barFill, { width: `${Math.max(pct, 2)}%`, backgroundColor: fill }]} />
    </View>
  );
}

export function Row({
  label,
  value,
  dim,
  mono,
}: {
  label: string;
  value: ReactNode;
  dim?: boolean;
  mono?: boolean;
}) {
  return (
    <View style={styles.row}>
      <Text style={styles.rowLabel}>{label}</Text>
      {typeof value === "string" || typeof value === "number" ? (
        <Text
          style={[
            styles.rowValue,
            dim && { color: colors.textDim },
            mono && { fontFamily: monoFont, fontSize: 12 },
          ]}
          numberOfLines={2}
        >
          {value}
        </Text>
      ) : (
        value
      )}
    </View>
  );
}

const toneColors: Record<Tone, string> = {
  ok: colors.ok,
  warn: colors.warn,
  danger: colors.danger,
  busy: colors.busy,
  dim: colors.textDim,
};

export function Badge({ label, tone = "dim" }: { label: string; tone?: Tone }) {
  const color = toneColors[tone];
  return (
    <View style={[styles.badge, { borderColor: color + "38", backgroundColor: color + "14" }]}>
      <View style={[styles.badgeDot, { backgroundColor: color }]} />
      <Text style={[styles.badgeText, { color }]}>{label}</Text>
    </View>
  );
}

export function Button({
  title,
  onPress,
  tone = "accent",
  disabled,
  loading,
  small,
}: {
  title: string;
  onPress: () => void;
  tone?: "accent" | "danger" | "ghost";
  disabled?: boolean;
  loading?: boolean;
  small?: boolean;
}) {
  const backgroundColor =
    tone === "accent"
      ? colors.accent
      : tone === "danger"
        ? colors.dangerDim
        : colors.cardSoft;
  const borderColor =
    tone === "danger"
      ? colors.danger + "55"
      : tone === "ghost"
        ? colors.cardBorderStrong
        : colors.accent;
  const textColor =
    tone === "accent" ? colors.accentInk : tone === "danger" ? colors.danger : colors.text;

  return (
    <Pressable
      onPress={() => {
        tapHaptic(tone === "danger" ? "medium" : "light");
        onPress();
      }}
      disabled={disabled || loading}
      style={({ pressed }) => [
        styles.button,
        small && styles.buttonSmall,
        {
          backgroundColor,
          borderColor,
          opacity: disabled ? 0.38 : pressed ? 0.78 : 1,
        },
      ]}
    >
      {loading ? (
        <ActivityIndicator color={textColor} size="small" />
      ) : (
        <Text style={[styles.buttonText, small && styles.buttonTextSmall, { color: textColor }]}>
          {title}
        </Text>
      )}
    </Pressable>
  );
}

export function ErrorBox({ error }: { error: unknown }) {
  if (!error) return null;
  const message = error instanceof Error ? error.message : String(error);
  return (
    <View style={styles.errorBox}>
      <Ionicons name="alert-circle-outline" size={16} color={colors.danger} style={{ marginTop: 1 }} />
      <Text style={styles.errorText}>{message}</Text>
    </View>
  );
}

export function Empty({ text }: { text: string }) {
  return (
    <View style={styles.empty}>
      <Text style={styles.emptyText}>{text}</Text>
    </View>
  );
}

export function Loading() {
  return (
    <View style={styles.empty}>
      <ActivityIndicator color={colors.accent} size="small" />
    </View>
  );
}

export function SectionTitle({ children }: { children: ReactNode }) {
  return <Text style={styles.sectionTitle}>{children}</Text>;
}

export function ListItem({
  title,
  subtitle,
  right,
  onPress,
}: {
  title: string;
  subtitle?: string;
  right?: ReactNode;
  onPress?: () => void;
}) {
  return (
    <Pressable
      onPress={
        onPress
          ? () => {
              tapHaptic("select");
              onPress();
            }
          : undefined
      }
      disabled={!onPress}
      style={({ pressed }) => [
        styles.listItem,
        pressed && onPress ? { backgroundColor: colors.cardSoft } : null,
      ]}
    >
      <View style={{ flex: 1, minWidth: 0 }}>
        <Text style={styles.listTitle} numberOfLines={1}>
          {title}
        </Text>
        {subtitle ? (
          <Text style={styles.listSubtitle} numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {right}
      {onPress ? (
        <Ionicons name="chevron-forward" size={16} color={colors.textFaint} style={{ marginLeft: 2 }} />
      ) : null}
    </Pressable>
  );
}

export function Input({
  value,
  onChangeText,
  placeholder,
  secure,
  multiline,
  keyboardType,
}: {
  value: string;
  onChangeText: (text: string) => void;
  placeholder?: string;
  secure?: boolean;
  multiline?: boolean;
  keyboardType?: "default" | "numeric" | "email-address" | "url";
}) {
  return (
    <TextInput
      style={[formStyles.input, multiline && formStyles.inputMultiline]}
      value={value}
      onChangeText={onChangeText}
      placeholder={placeholder}
      placeholderTextColor={colors.textFaint}
      secureTextEntry={secure}
      autoCapitalize="none"
      autoCorrect={false}
      multiline={multiline}
      keyboardType={keyboardType}
    />
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, backgroundColor: colors.bg },
  screenContent: {
    paddingHorizontal: spacing.lg,
    paddingTop: spacing.md,
    paddingBottom: 40,
    gap: spacing.md,
  },
  card: {
    backgroundColor: colors.card,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    padding: spacing.lg,
    gap: spacing.sm,
  },
  cardTitle: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "600",
    letterSpacing: -0.2,
    marginBottom: 2,
  },
  insetGroup: {
    backgroundColor: colors.card,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    overflow: "hidden",
  },
  insetDivider: {
    height: hairline,
    backgroundColor: colors.cardBorder,
    marginLeft: spacing.lg,
  },
  groupedRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    paddingHorizontal: spacing.lg,
    paddingVertical: 12,
  },
  groupedIconWrap: {
    width: 24,
    alignItems: "center",
    justifyContent: "center",
  },
  ledger: {
    flexDirection: "row",
    backgroundColor: colors.card,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    overflow: "hidden",
  },
  ledgerCell: {
    flex: 1,
    paddingHorizontal: spacing.md,
    paddingVertical: 13,
    gap: 4,
  },
  ledgerDivider: {
    width: hairline,
    backgroundColor: colors.cardBorder,
  },
  ledgerLabel: {
    color: colors.textDim,
    fontSize: 11.5,
    fontWeight: "500",
  },
  ledgerValueRow: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: 3,
  },
  ledgerValue: {
    fontSize: 22,
    fontWeight: "700",
    letterSpacing: -0.6,
    fontVariant: ["tabular-nums"],
  },
  ledgerSuffix: {
    color: colors.textFaint,
    fontSize: 12,
    fontWeight: "500",
    fontVariant: ["tabular-nums"],
  },
  ledgerCaption: {
    fontSize: 11,
    fontWeight: "500",
    fontVariant: ["tabular-nums"],
  },
  segmented: {
    flexDirection: "row",
    backgroundColor: colors.bgElevated,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    padding: 3,
    gap: 2,
  },
  segmentItem: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingVertical: 6,
    paddingHorizontal: 8,
    borderRadius: 7,
  },
  segmentActive: {
    backgroundColor: colors.cardSoft,
    borderWidth: hairline,
    borderColor: colors.cardBorderStrong,
  },
  segmentLabel: {
    color: colors.textDim,
    fontSize: 12.5,
    fontWeight: "500",
  },
  segmentLabelActive: {
    color: colors.text,
    fontWeight: "600",
  },
  barTrack: {
    height: 4,
    borderRadius: 2,
    backgroundColor: colors.cardSoft,
    overflow: "hidden",
  },
  barFill: {
    height: 4,
    borderRadius: 2,
  },
  row: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    gap: spacing.md,
    paddingVertical: 2,
  },
  rowLabel: { color: colors.textDim, fontSize: 13 },
  rowValue: {
    color: colors.text,
    fontSize: 13,
    fontWeight: "500",
    flexShrink: 1,
    textAlign: "right",
    fontVariant: ["tabular-nums"],
  },
  badge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 5,
    borderWidth: 1,
    borderRadius: 999,
    paddingHorizontal: 8,
    paddingVertical: 3,
    alignSelf: "flex-start",
  },
  badgeDot: { width: 5, height: 5, borderRadius: 2.5 },
  badgeText: { fontSize: 11, fontWeight: "600" },
  button: {
    borderRadius: radius.md,
    borderWidth: 1,
    paddingVertical: 12,
    paddingHorizontal: spacing.lg,
    alignItems: "center",
    justifyContent: "center",
  },
  buttonSmall: { paddingVertical: 7, paddingHorizontal: spacing.md, borderRadius: 9 },
  buttonText: { fontSize: 14.5, fontWeight: "600", letterSpacing: -0.1 },
  buttonTextSmall: { fontSize: 12.5 },
  errorBox: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: spacing.sm,
    backgroundColor: colors.dangerDim,
    borderColor: colors.danger + "44",
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.md,
  },
  errorText: { color: colors.danger, fontSize: 13, flex: 1, lineHeight: 18 },
  empty: { padding: spacing.xl, alignItems: "center" },
  emptyText: { color: colors.textDim, fontSize: 13.5, textAlign: "center" },
  sectionTitle: {
    color: colors.textDim,
    fontSize: 11.5,
    fontWeight: "600",
    textTransform: "uppercase",
    letterSpacing: 0.7,
    marginTop: spacing.xs,
    marginLeft: 4,
  },
  listItem: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    backgroundColor: colors.card,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    paddingHorizontal: spacing.lg,
    paddingVertical: 13,
  },
  listTitle: { color: colors.text, fontSize: 14.5, fontWeight: "600", letterSpacing: -0.15 },
  listSubtitle: { color: colors.textDim, fontSize: 12, marginTop: 2 },
});

const formStyles = StyleSheet.create({
  input: {
    backgroundColor: colors.bgElevated,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: radius.md,
    paddingHorizontal: spacing.md,
    paddingVertical: 12,
    color: colors.text,
    fontSize: 14.5,
  },
  inputMultiline: { minHeight: 90, textAlignVertical: "top" },
});
