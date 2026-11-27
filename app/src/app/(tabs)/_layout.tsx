import { Ionicons } from "@expo/vector-icons";
import { Tabs } from "expo-router";
import { colors } from "../../lib/theme";

export default function TabsLayout() {
  return (
    <Tabs
      screenOptions={{
        headerStyle: { backgroundColor: colors.bg },
        headerTintColor: colors.text,
        headerShadowVisible: false,
        tabBarStyle: { backgroundColor: colors.card, borderTopColor: colors.cardBorder },
        tabBarActiveTintColor: colors.accent,
        tabBarInactiveTintColor: colors.textDim,
      }}
    >
      <Tabs.Screen
        name="index"
        options={{
          title: "Tổng quan",
          tabBarIcon: ({ color, size }) => <Ionicons name="speedometer-outline" color={color as string} size={size} />,
        }}
      />
      <Tabs.Screen
        name="services"
        options={{
          title: "Dịch vụ",
          tabBarIcon: ({ color, size }) => <Ionicons name="cube-outline" color={color as string} size={size} />,
        }}
      />
      <Tabs.Screen
        name="backups"
        options={{
          title: "Sao lưu",
          tabBarIcon: ({ color, size }) => <Ionicons name="archive-outline" color={color as string} size={size} />,
        }}
      />
      <Tabs.Screen
        name="system"
        options={{
          title: "Hệ thống",
          tabBarIcon: ({ color, size }) => <Ionicons name="settings-outline" color={color as string} size={size} />,
        }}
      />
    </Tabs>
  );
}
