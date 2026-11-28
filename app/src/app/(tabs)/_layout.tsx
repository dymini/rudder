import { Ionicons } from "@expo/vector-icons";
import { Tabs } from "expo-router";
import { Platform } from "react-native";
import { colors, hairline, tapHaptic } from "../../lib/theme";

export default function TabsLayout() {
  return (
    <Tabs
      screenOptions={{
        headerStyle: {
          backgroundColor: colors.bg,
          borderBottomWidth: hairline,
          borderBottomColor: colors.cardBorder,
        },
        headerTintColor: colors.text,
        headerShadowVisible: false,
        headerTitleStyle: { fontWeight: "600", fontSize: 17 },
        tabBarStyle: {
          backgroundColor: colors.bgElevated,
          borderTopWidth: hairline,
          borderTopColor: colors.cardBorder,
          height: Platform.OS === "ios" ? 84 : 66,
          paddingTop: 6,
          paddingBottom: Platform.OS === "ios" ? 26 : 10,
        },
        tabBarLabelStyle: {
          fontSize: 10.5,
          fontWeight: "600",
          letterSpacing: 0.1,
          lineHeight: 14,
        },
        tabBarActiveTintColor: colors.accent,
        tabBarInactiveTintColor: colors.textFaint,
      }}
      screenListeners={{
        tabPress: () => tapHaptic("select"),
      }}
    >
      <Tabs.Screen
        name="index"
        options={{
          title: "Tổng quan",
          tabBarIcon: ({ color, focused }) => (
            <Ionicons
              name={focused ? "grid" : "grid-outline"}
              color={color as string}
              size={21}
            />
          ),
        }}
      />
      <Tabs.Screen
        name="services"
        options={{
          title: "Dịch vụ",
          tabBarIcon: ({ color, focused }) => (
            <Ionicons
              name={focused ? "cube" : "cube-outline"}
              color={color as string}
              size={21}
            />
          ),
        }}
      />
      <Tabs.Screen
        name="backups"
        options={{
          title: "Sao lưu",
          tabBarIcon: ({ color, focused }) => (
            <Ionicons
              name={focused ? "archive" : "archive-outline"}
              color={color as string}
              size={21}
            />
          ),
        }}
      />
      <Tabs.Screen
        name="system"
        options={{
          title: "Hệ thống",
          tabBarIcon: ({ color, focused }) => (
            <Ionicons
              name={focused ? "options" : "options-outline"}
              color={color as string}
              size={21}
            />
          ),
        }}
      />
    </Tabs>
  );
}
