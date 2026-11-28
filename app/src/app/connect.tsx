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
import { Button, DyminiMark, ErrorBox } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, hairline, monoFont, radius, spacing, tapHaptic } from "../lib/theme";

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
      <ScrollView
        contentContainerStyle={styles.content}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        <View style={styles.brandBlock}>
          <DyminiMark size={48} />
          <Text style={styles.title}>Dymini Panel</Text>
          <Text style={styles.subtitle}>
            Giám sát máy chủ, triển khai dịch vụ và xử lý sự cố ngay trên thiết bị của bạn.
          </Text>
        </View>

        <View style={styles.formCard}>
          <View style={styles.fieldGroup}>
            <Text style={styles.label}>Địa chỉ panel</Text>
            <View style={styles.inputShell}>
              <Ionicons name="globe-outline" color={colors.textFaint} size={16} />
              <TextInput
                style={styles.input}
                value={panelUrl}
                onChangeText={setPanelUrl}
                placeholder="https://panel.example.com"
                placeholderTextColor={colors.textFaint}
                autoCapitalize="none"
                autoCorrect={false}
                keyboardType="url"
                returnKeyType="go"
              />
            </View>
          </View>

          <ErrorBox error={error} />

          <Button
            title="Đăng nhập qua trình duyệt"
            loading={busy}
            disabled={!panelUrl.trim()}
            onPress={() => run(() => signIn(panelUrl))}
          />

          <Text style={styles.hint}>
            Mở trình duyệt bảo mật của hệ thống để xác thực OAuth và tự động quay lại ứng dụng.
          </Text>

          <View style={styles.divider} />

          <Pressable
            onPress={() => {
              tapHaptic("select");
              setShowToken((v) => !v);
            }}
            style={styles.tokenToggleWrap}
          >
            <Text style={styles.tokenToggle}>
              {showToken ? "Ẩn đăng nhập bằng API token" : "Đăng nhập bằng API token"}
            </Text>
            <Ionicons
              name={showToken ? "chevron-up" : "chevron-down"}
              color={colors.textDim}
              size={15}
            />
          </Pressable>

          {showToken ? (
            <View style={{ gap: spacing.md }}>
              <View style={styles.inputShell}>
                <Ionicons name="key-outline" color={colors.textFaint} size={16} />
                <TextInput
                  style={[styles.input, { fontFamily: monoFont, fontSize: 13 }]}
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
                title="Kết nối bằng token"
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
    paddingHorizontal: spacing.xl,
    paddingVertical: spacing.xxl,
    gap: spacing.xl,
  },
  brandBlock: {
    gap: spacing.sm,
  },
  title: {
    color: colors.text,
    fontSize: 28,
    fontWeight: "700",
    letterSpacing: -0.7,
    marginTop: spacing.xs,
  },
  subtitle: {
    color: colors.textDim,
    fontSize: 14.5,
    lineHeight: 21,
  },
  formCard: {
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: radius.xl,
    padding: spacing.lg,
    gap: spacing.md,
  },
  fieldGroup: {
    gap: 6,
  },
  label: {
    color: colors.textSoft,
    fontSize: 12.5,
    fontWeight: "600",
  },
  inputShell: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    backgroundColor: colors.bgElevated,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: radius.md,
    paddingHorizontal: spacing.md,
  },
  input: {
    flex: 1,
    paddingVertical: 12,
    color: colors.text,
    fontSize: 14.5,
  },
  hint: {
    color: colors.textFaint,
    fontSize: 12,
    lineHeight: 17,
  },
  divider: {
    height: hairline,
    backgroundColor: colors.cardBorder,
    marginVertical: 2,
  },
  tokenToggleWrap: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingVertical: 2,
  },
  tokenToggle: {
    color: colors.textSoft,
    fontSize: 13.5,
    fontWeight: "500",
  },
});
