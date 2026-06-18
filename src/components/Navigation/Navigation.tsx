import { NavLink } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  BarChart3,
  Network,
  Cable,
  HelpCircle,
  Settings,
} from "lucide-react";

const navItems = [
  { path: "/", labelKey: "nav.dashboard", icon: BarChart3 },
  { path: "/traffic", labelKey: "nav.traffic", icon: Network },
  { path: "/connections", labelKey: "nav.connections", icon: Cable },
  { path: "/emergency", labelKey: "nav.emergency", icon: HelpCircle },
  { path: "/settings", labelKey: "nav.settings", icon: Settings },
];

export default function Navigation() {
  const { t } = useTranslation();
  return (
    <nav className="w-56 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col shrink-0">
      <div className="p-4 border-b border-gray-200 dark:border-gray-700">
        <h1 className="text-xl font-bold text-gray-800 dark:text-gray-100 flex items-center gap-2">
          <Network className="w-6 h-6 text-blue-600" />
          NetAssist
        </h1>
      </div>

      <div className="flex-1 py-4">
        <ul className="space-y-1 px-2">
          {navItems.map((item) => (
            <li key={item.path}>
              <NavLink
                to={item.path}
                className={({ isActive }) =>
                  `flex items-center gap-3 px-3 py-2.5 rounded-lg transition-colors ${
                    isActive
                      ? "bg-blue-50 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 font-medium"
                      : "text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-100"
                  }`
                }
              >
                <item.icon className="w-5 h-5" />
                <span>{t(item.labelKey)}</span>
              </NavLink>
            </li>
          ))}
        </ul>
      </div>
    </nav>
  );
}
