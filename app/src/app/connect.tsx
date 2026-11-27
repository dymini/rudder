import { Ionicons } from "@expo/vector-icons";
import { router } from "expo-router";
import { useState } from "react";
import {
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { Button, ErrorBox } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, spacing } from "../lib/theme";

export default function Connect() {
  const { signIn, signInWithToken } = useAuth();
  const [panelUrl, setPanelUrl] = useState("");
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<void>) {
    setError(null);
    setBusy(true);
    try {
      await action();
      router.replace("/(tabs)");
    } catch (err) {
      if (!(err instanceof Error && err.message.includes("hủy"))) {
        setError(err);
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <KeyboardAvoidingView
      style={{ flex: 1, backgroundColor: colors.bg }}
      behavior={Platform.OS === "ios" ? "padding" : undefined}
    >
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
        <View style={styles.heroGlow} />
        <View style={styles.brandOrbit}>
          <View style={styles.brandBadge}>
            <Ionicons name="compass" color={colors.accentBright} size={42} />
          </View>
        </View>
        <Text style={styles.title}>Dymini Panel</Text>
        <Text style={styles.subtitle}>Bảng điều khiển hạm đội của bạn — nhanh, an toàn và luôn trong tầm tay.</Text>

        <View style={styles.formCard}>
          <Text style={styles.label}>Địa chỉ panel</Text>
          <View style={styles.inputShell}>
            <Ionicons name="link-outline" color={colors.textDim} size={18} />
            <TextInput
              style={styles.input}
              value={panelUrl}
              onChangeText={setPanelUrl}
              placeholder="https://dymini-panel.example.com"
              placeholderTextColor={colors.textFaint}
              autoCapitalize="none"
              autoCorrect={false}
              keyboardType="url"
            />
          </View>

          <ErrorBox error={error} />

          <Button
            title="Đăng nhập qua trình duyệt"
            loading={busy}
            disabled={!panelUrl.trim()}
            onPress={() => run(() => signIn(panelUrl))}
          />
          <Text style={styles.hint}>
            Phiên đăng nhập mở trong trình duyệt bảo mật của hệ thống, sau đó tự động quay lại app.
          </Text>

          <Pressable onPress={() => setShowToken((v) => !v)} style={styles.tokenToggleWrap}>
            <Text style={styles.tokenToggle}>{showToken ? "Ẩn đăng nhập bằng API token" : "Dùng API token"}</Text>
            <Ionicons name={showToken ? "chevron-up" : "chevron-down"} color={colors.accentBright} size={15} />
          </Pressable>
          {showToken ? (
            <View style={{ gap: spacing.md }}>
              <View style={styles.inputShell}>
                <Ionicons name="key-outline" color={colors.textDim} size={18} />
                <TextInput
                  style={styles.input}
                  value={token}
                  onChangeText={setToken}
                  placeholder="dymini_panel_token_…"
                  placeholderTextColor={colors.textFaint}
                  autoCapitalize="none"
                  autoCorrect={false}
                  secureTextEntry
                />
              </View>
              <Button
                title="Đăng nhập bằng token"
                tone="ghost"
                loading={busy}
                disabled={!panelUrl.trim() || !token.trim()}
                onPress={() => run(() => signInWithToken(panelUrl, token))}
              />
            </View>
          ) : null}
        </View>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  content: {
    flexGrow: 1,
    justifyContent: "center",
    padding: spacing.xl,
    gap: spacing.lg,
  },
  heroGlow: {
    position: "absolute",
    top: 30,
    alignSelf: "center",
    width: 260,
    height: 260,
    borderRadius: 130,
    backgroundColor: colors.accentDim,
    opacity: 0.42,
  },
  brandOrbit: {
    alignSelf: "center",
    width: 104,
    height: 104,
    borderRadius: 52,
    borderWidth: 1,
    borderColor: colors.accent + "44",
    alignItems: "center",
    justifyContent: "center",
  },
  brandBadge: {
    width: 76,
    height: 76,
    borderRadius: 26,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.cardSoft,
    borderWidth: 1,
    borderColor: colors.accent + "66",
    shadowColor: colors.accent,
    shadowOpacity: 0.35,
    shadowRadius: 24,
    shadowOffset: { width: 0, height: 8 },
  },
  title: { color: colors.text, fontSize: 38, fontWeight: "800", letterSpacing: -1, textAlign: "center" },
  subtitle: { color: colors.textDim, fontSize: 15, lineHeight: 22, textAlign: "center", marginHorizontal: spacing.lg },
  formCard: {
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 26,
    padding: spacing.xl,
    gap: spacing.md,
    shadowColor: "#000",
    shadowOpacity: 0.32,
    shadowRadius: 24,
    shadowOffset: { width: 0, height: 14 },
    elevation: 8,
  },
  label: { color: colors.textDim, fontSize: 13, fontWeight: "700" },
  inputShell: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    backgroundColor: colors.bgElevated,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 16,
    paddingHorizontal: spacing.md,
  },
  input: {
    flex: 1,
    paddingVertical: 14,
    color: colors.text,
    fontSize: 15,
  },
  hint: { color: colors.textDim, fontSize: 12, lineHeight: 17, textAlign: "center" },
  tokenToggleWrap: { flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 4, paddingVertical: spacing.xs },
  tokenToggle: { color: colors.accentBright, fontSize: 14, fontWeight: "700", textAlign: "center" },
});
