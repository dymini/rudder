import React from "react";
import { Text, View, ScrollView } from "react-native";
import { colors, spacing } from "../lib/theme";

type Props = { children: React.ReactNode };
type State = { error: Error | null };

export class ErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <ScrollView style={{ flex: 1, backgroundColor: colors.bg }} contentContainerStyle={{ padding: spacing.lg }}>
        <View style={{ backgroundColor: colors.card, borderRadius: 12, padding: spacing.lg, borderWidth: 1, borderColor: colors.danger }}>
          <Text style={{ color: colors.danger, fontSize: 17, fontWeight: "700", marginBottom: spacing.sm }}>
            Đã xảy ra lỗi hiển thị
          </Text>
          <Text style={{ color: colors.text, fontSize: 13, marginBottom: spacing.sm }}>
            Chụp màn hình này và gửi cho nhà phát triển để được sửa.
          </Text>
          <Text selectable style={{ color: colors.textDim, fontSize: 11, fontFamily: "Menlo" }}>
            {String(error?.message ?? error)}
            {"\n\n"}
            {String(error?.stack ?? "").slice(0, 2000)}
          </Text>
          <Text
            style={{ color: colors.accent, marginTop: spacing.md, fontSize: 15, fontWeight: "600" }}
            onPress={() => this.setState({ error: null })}
          >
            Thử lại
          </Text>
        </View>
      </ScrollView>
    );
  }
}
