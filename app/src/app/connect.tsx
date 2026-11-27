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
import { useAuth } from "../lib/auth";
import { colors, spacing } from "../lib/theme";
import { Button, ErrorBox } from "../components/ui";

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
      if (err instanceof Error && err.message.includes("hủy")) {
        // user cancelled the sign-in sheet: no error box needed
      } else {
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
        <Text style={styles.logo}>⚓</Text>
        <Text style={styles.title}>Rudder</Text>
        <Text style={styles.subtitle}>Điều khiển panel Rudder của bạn từ điện thoại.</Text>

        <Text style={styles.label}>Địa chỉ panel</Text>
        <TextInput
          style={styles.input}
          value={panelUrl}
          onChangeText={setPanelUrl}
          placeholder="https://rudder.example.com"
          placeholderTextColor={colors.textDim}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
        />

        <ErrorBox error={error} />

        <Button
          title="Đăng nhập qua trình duyệt"
          loading={busy}
          disabled={!panelUrl.trim()}
          onPress={() => run(() => signIn(panelUrl))}
        />
        <Text style={styles.hint}>
          Mở trang đăng nhập của panel trong một phiên trình duyệt bảo mật, bạn phê duyệt quyền rồi
          quay lại app.
        </Text>

        <Pressable onPress={() => setShowToken((v) => !v)}>
          <Text style={styles.tokenToggle}>{showToken ? "Ẩn đăng nhập bằng API token" : "Dùng API token"}</Text>
        </Pressable>
        {showToken ? (
          <View style={{ gap: spacing.md }}>
            <TextInput
              style={styles.input}
              value={token}
              onChangeText={setToken}
              placeholder="rudder_token_…"
              placeholderTextColor={colors.textDim}
              autoCapitalize="none"
              autoCorrect={false}
              secureTextEntry
            />
            <Button
              title="Đăng nhập bằng token"
              tone="ghost"
              loading={busy}
              disabled={!panelUrl.trim() || !token.trim()}
              onPress={() => run(() => signInWithToken(panelUrl, token))}
            />
          </View>
        ) : null}
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  content: {
    flexGrow: 1,
    justifyContent: "center",
    padding: spacing.xl,
    gap: spacing.md,
  },
  logo: { fontSize: 44, textAlign: "center" },
  title: { color: colors.text, fontSize: 32, fontWeight: "700", textAlign: "center" },
  subtitle: { color: colors.textDim, fontSize: 15, textAlign: "center", marginBottom: spacing.lg },
  label: { color: colors.textDim, fontSize: 13, fontWeight: "600" },
  input: {
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 10,
    paddingHorizontal: spacing.md,
    paddingVertical: 12,
    color: colors.text,
    fontSize: 15,
  },
  hint: { color: colors.textDim, fontSize: 12, textAlign: "center" },
  tokenToggle: { color: colors.accent, fontSize: 14, textAlign: "center", paddingVertical: spacing.sm },
});
