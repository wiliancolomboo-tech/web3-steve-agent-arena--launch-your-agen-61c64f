import axios from "axios";

export interface TelemetryAlert {
  title: string;
  matchId: string;
  agentPubkey: string;
  opponentPubkey?: string;
  actionTaken: string;
  pnlLamports?: number;
  status: "INFO" | "SUCCESS" | "WARNING" | "CRITICAL";
}

export class EnterpriseTelemetry {
  private webhookUrl: string | undefined;

  constructor() {
    this.webhookUrl = process.env.TELEMETRY_WEBHOOK_URL;
  }

  public async sendAlert(alert: TelemetryAlert): Promise<void> {
    const timestamp = new Date().toISOString();
    console.log(`[${timestamp}] [${alert.status}] ${alert.title} | Match: ${alert.matchId}`);

    if (!this.webhookUrl) {
      return;
    }

    try {
      const payload = {
        username: "Aegis-Steve Sentinel",
        embeds: [
          {
            title: `🛡️ Aegis-Steve Arena Alert: ${alert.title}`,
            color: alert.status === "SUCCESS" ? 0x00ff00 : alert.status === "CRITICAL" ? 0xff0000 : 0x0099ff,
            fields: [
              { name: "Match ID", value: alert.matchId, inline: true },
              { name: "Agent Key", value: `\`${alert.agentPubkey.slice(0, 8)}...\``, inline: true },
              { name: "Action", value: alert.actionTaken, inline: true },
              { name: "Status", value: alert.status, inline: true },
              { name: "PnL (SOL)", value: alert.pnlLamports ? (alert.pnlLamports / 1e9).toFixed(4) : "0.00", inline: true }
            ],
            footer: { text: "Steve Agent Arena | Solana Mainnet/Devnet" },
            timestamp
          }
        ]
      };

      await axios.post(this.webhookUrl, payload, { timeout: 4000 });
    } catch (err: any) {
      console.error("Failed to forward telemetry to enterprise webhook:", err.message);
    }
  }
}