import { type FC, useState } from "react";
import { useTranslation } from "react-i18next";
import { Network } from "lucide-react";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { Input } from "../../ui/Input";
import { Button } from "../../ui/Button";
import { Alert } from "../../ui/Alert";
import { ResetButton } from "../../ui/ResetButton";
import { useSettings } from "../../../hooks/useSettings";
import { commands } from "@/bindings";
import type { TranscriptionBackend } from "@/bindings";

type TestStatus = "idle" | "testing" | "success" | "error";

const DEFAULT_LISTEN_ADDR = "127.0.0.1:8080";

export const TranscriptionServerSettings: FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const backend =
    (getSetting("transcription_backend") as TranscriptionBackend | undefined) ??
    "local";
  const serverUrl = (getSetting("remote_server_url") as string | null) ?? "";
  const serverToken =
    (getSetting("remote_server_token") as string | null) ?? "";
  const listenAddr =
    (getSetting("remote_server_listen_addr") as string | undefined) ??
    DEFAULT_LISTEN_ADDR;

  const [urlDraft, setUrlDraft] = useState(serverUrl);
  const [tokenDraft, setTokenDraft] = useState(serverToken);
  const [testStatus, setTestStatus] = useState<TestStatus>("idle");
  const [testMessage, setTestMessage] = useState("");

  const backendOptions: DropdownOption[] = [
    { value: "local", label: t("settings.transcriptionServer.backend.local") },
    {
      value: "remote",
      label: t("settings.transcriptionServer.backend.remote"),
    },
  ];

  const handleBackendChange = async (value: string) => {
    await updateSetting("transcription_backend", value as TranscriptionBackend);
    setTestStatus("idle");
    setTestMessage("");
  };

  const commitUrl = async () => {
    if (urlDraft !== serverUrl) {
      await updateSetting(
        "remote_server_url",
        urlDraft.trim() === "" ? null : urlDraft.trim(),
      );
    }
  };
  const commitToken = async () => {
    if (tokenDraft !== serverToken) {
      await updateSetting(
        "remote_server_token",
        tokenDraft.trim() === "" ? null : tokenDraft.trim(),
      );
    }
  };

  const handleTest = async () => {
    setTestStatus("testing");
    setTestMessage("");
    const result = await commands.testRemoteServerConnection(
      urlDraft.trim(),
      tokenDraft.trim() === "" ? null : tokenDraft.trim(),
    );
    if (result.status === "ok") {
      const h = result.data;
      setTestStatus("success");
      setTestMessage(
        t("settings.transcriptionServer.test.success", {
          model: h.model ?? "—",
          loaded: h.loaded,
        }),
      );
    } else {
      setTestStatus("error");
      setTestMessage(result.error);
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.transcriptionServer.title")}>
        <SettingContainer
          title={t("settings.transcriptionServer.backend.label")}
          description={t("settings.transcriptionServer.backend.description")}
          descriptionMode="tooltip"
          grouped
          layout="horizontal"
        >
          <Dropdown
            options={backendOptions}
            selectedValue={backend}
            onSelect={handleBackendChange}
            disabled={isUpdating("transcription_backend")}
          />
        </SettingContainer>

        {backend === "remote" && (
          <>
            <SettingContainer
              title={t("settings.transcriptionServer.serverUrl.label")}
              description={t(
                "settings.transcriptionServer.serverUrl.description",
              )}
              descriptionMode="tooltip"
              grouped
              layout="stacked"
            >
              <div className="flex items-center gap-2">
                <Input
                  className="flex-1"
                  value={urlDraft}
                  placeholder={t(
                    "settings.transcriptionServer.serverUrl.placeholder",
                  )}
                  onChange={(e) => setUrlDraft(e.target.value)}
                  onBlur={commitUrl}
                  spellCheck={false}
                  autoComplete="off"
                />
                <ResetButton
                  onClick={() => {
                    setUrlDraft("");
                    void updateSetting("remote_server_url", null);
                  }}
                />
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("settings.transcriptionServer.token.label")}
              description={t("settings.transcriptionServer.token.description")}
              descriptionMode="tooltip"
              grouped
              layout="stacked"
            >
              <div className="flex items-center gap-2">
                <Input
                  className="flex-1"
                  type="password"
                  value={tokenDraft}
                  placeholder={t(
                    "settings.transcriptionServer.token.placeholder",
                  )}
                  onChange={(e) => setTokenDraft(e.target.value)}
                  onBlur={commitToken}
                  spellCheck={false}
                  autoComplete="off"
                />
                <ResetButton
                  onClick={() => {
                    setTokenDraft("");
                    void updateSetting("remote_server_token", null);
                  }}
                />
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("settings.transcriptionServer.test.button")}
              description={t("settings.transcriptionServer.test.description")}
              descriptionMode="tooltip"
              grouped
              layout="horizontal"
            >
              <Button
                variant="primary-soft"
                size="sm"
                onClick={handleTest}
                disabled={testStatus === "testing" || urlDraft.trim() === ""}
              >
                {testStatus === "testing"
                  ? t("settings.transcriptionServer.test.testing")
                  : t("settings.transcriptionServer.test.button")}
              </Button>
            </SettingContainer>

            {testStatus === "success" && (
              <Alert variant="success" contained>
                {testMessage}
              </Alert>
            )}
            {testStatus === "error" && (
              <Alert variant="error" contained>
                {testMessage}
              </Alert>
            )}

            <Alert variant="info" contained>
              {t("settings.transcriptionServer.remoteNotice")}
            </Alert>
          </>
        )}
      </SettingsGroup>

      <SettingsGroup title={t("settings.transcriptionServer.serverMode.title")}>
        <SettingContainer
          title={t("settings.transcriptionServer.serverMode.listenAddr.label")}
          description={t(
            "settings.transcriptionServer.serverMode.listenAddr.description",
          )}
          descriptionMode="tooltip"
          grouped
          layout="horizontal"
        >
          <Input
            value={listenAddr}
            onChange={(e) =>
              updateSetting("remote_server_listen_addr", e.target.value)
            }
            spellCheck={false}
            autoComplete="off"
          />
        </SettingContainer>
        <div className="px-4 py-3 text-xs text-text/60 flex items-start gap-2">
          <Network className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{t("settings.transcriptionServer.serverMode.setupHint")}</span>
        </div>
      </SettingsGroup>
    </div>
  );
};
