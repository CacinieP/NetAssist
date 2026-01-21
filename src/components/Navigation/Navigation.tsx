import { NavLink } from "react-router-dom";
import {
  BarChart3,
  Network,
  Cable,
  HelpCircle,
  Settings,
} from "lucide-react";

const navItems = [
  { path: "/", label: "仪表盘", icon: BarChart3 },
  { path: "/traffic", label: "流量监控", icon: Network },
  { path: "/connections", label: "连接管理", icon: Cable },
  { path: "/emergency", label: "断网急救", icon: HelpCircle },
  { path: "/settings", label: "设置", icon: Settings },
];

export default function Navigation() {
  return (
    <nav className="w-56 bg-white border-r border-gray-200 flex flex-col shrink-0">
      <div className="p-4 border-b border-gray-200">
        <h1 className="text-xl font-bold text-gray-800 flex items-center gap-2">
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
                      ? "bg-blue-50 text-blue-700 font-medium"
                      : "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
                  }`
                }
              >
                <item.icon className="w-5 h-5" />
                <span>{item.label}</span>
              </NavLink>
            </li>
          ))}
        </ul>
      </div>
    </nav>
  );
}
