import { Ionicons } from "@expo/vector-icons";
import { Tabs } from "expo-router";
import { Platform } from "react-native";
import { colors } from "../../lib/theme";

export default function TabsLayout() {
  return (
    <Tabs
      screenOptions={{
        headerStyle: { backgroundColor: colors.bg },
        headerTintColor: colors.text,
        headerShadowVisible: false,
        headerTitleStyle: { fontWeight: "700", fontSize: 20 },
        tabBarStyle: {
          position: "absolute",
          left: 14,
          right: 14,
          bottom: Platform.OS === "ios" ? 18 : 12,
          height: 68,
          borderRadius: 26,
          backgroundColor: colors.card,
          borderTopWidth: 1,
          borderTopColor: colors.cardBorder,
          shadowColor: "#000",
          shadowOpacity: 0.35,
          shadowRadius: 18,
          shadowOffset: { width: 0, height: 10 },
          elevation: 12,
        },
        tabBarItemStyle: { paddingVertical: 7 },
        tabBarLabelStyle: { fontSize: 11, fontWeight: "600" },
        tabBarActiveTintColor: colors.accentBright,
        tabBarInactiveTintColor: colors.textDim,
      }}
    >
      <Tabs.Screen
        name="index"
        options={{
          title: "Tổng quan",
          tabBarIcon: ({ color, size, focused }) => (
            <Ionicons name={focused ? "speedometer" : "speedometer-outline"} color={color as string} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="services"
        options={{
          title: "Dịch vụ",
          tabBarIcon: ({ color, size, focused }) => (
            <Ionicons name={focused ? "cube" : "cube-outline"} color={color as string} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="backups"
        options={{
          title: "Sao lưu",
          tabBarIcon: ({ color, size, focused }) => (
            <Ionicons name={focused ? "archive" : "archive-outline"} color={color as string} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="system"
        options={{
          title: "Hệ thống",
          tabBarIcon: ({ color, size, focused }) => (
            <Ionicons name={focused ? "settings" : "settings-outline"} color={color as string} size={size} />
          ),
        }}
      />
    </Tabs>
  );
}
