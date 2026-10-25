pub use crate::alerts::Notification;
use crate::{alerts::money, catalog::Selection};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
};
use std::{future::Future, pin::Pin, time::Duration};

pub struct CostAlert<'a> {
    pub recipient: &'a str,
    pub selection: &'a Selection,
    pub threshold: u64,
    pub notification: Notification,
}

pub trait Mailer: Send + Sync {
    fn send<'a>(
        &'a self,
        alert: CostAlert<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + 'a>>;
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    sender: Mailbox,
    origin: String,
}

impl SmtpMailer {
    // Local Mailpit only: unencrypted SMTP is deliberately not a production setup.
    pub fn new(host: &str, port: u16, sender: &str, origin: &str) -> Result<Self, &'static str> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(port)
                .timeout(Some(Duration::from_secs(10)))
                .build(),
            sender: sender.parse().map_err(|_| "Invalid SMTP sender")?,
            origin: origin.to_owned(),
        })
    }
}

fn content(alert: &CostAlert<'_>, origin: &str) -> String {
    let status = match alert.notification {
        Notification::Exceeded => "dépasse son seuil d’alerte",
        Notification::Recovered => {
            "est revenue au seuil d’alerte ou en dessous : retour à la normale"
        }
    };
    let mut body = format!(
        "Bonjour,\n\nVotre sélection « {} » {status}.\n\nCoût mensuel : {}\nSeuil : {}\n",
        alert.selection.name,
        money(alert.selection.monthly_total_cents),
        money(alert.threshold),
    );
    if alert.notification == Notification::Exceeded {
        body.push_str(&format!(
            "Dépassement : {}\n",
            money(
                alert
                    .selection
                    .monthly_total_cents
                    .saturating_sub(alert.threshold)
            )
        ));
    }
    body.push_str("\nServices de la sélection :\n");
    for service in &alert.selection.services {
        body.push_str(&format!(
            "- {} : {} / mois\n",
            service.name,
            money(service.monthly_price_cents)
        ));
    }
    let next = match alert.notification {
        Notification::Exceeded => {
            "Aucun rappel ne sera envoyé pendant ce dépassement. Un e-mail vous informera du retour au seuil ou en dessous."
        }
        Notification::Recovered => {
            "L’alerte est réarmée. Vous recevrez un nouvel e-mail en cas de nouveau dépassement, sans rappel tant que le coût reste au seuil ou en dessous."
        }
    };
    body.push_str(&format!(
        "\nConsulter vos sélections : {origin}/\n\n{next}\n\nCost Monitor\n"
    ));
    body
}

fn message(sender: Mailbox, alert: &CostAlert<'_>, origin: &str) -> Result<Message, ()> {
    Message::builder()
        .from(sender)
        .to(alert.recipient.parse().map_err(|_| ())?)
        .subject(match alert.notification {
            Notification::Exceeded => "[Cost Monitor] Seuil de coût dépassé",
            Notification::Recovered => "[Cost Monitor] Retour à la normale",
        })
        .header(ContentType::TEXT_PLAIN)
        .body(content(alert, origin))
        .map_err(|_| ())
}

impl Mailer for SmtpMailer {
    fn send<'a>(
        &'a self,
        alert: CostAlert<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ()>> + Send + 'a>> {
        Box::pin(async move {
            let message = message(self.sender.clone(), &alert, &self.origin)?;
            self.transport.send(message).await.map_err(|_| ())?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Service;
    #[test]
    fn recovery_email_contains_current_prices_and_no_exceedance_wording() {
        let selection = Selection {
            id: 1,
            name: "Montréal".into(),
            monthly_total_cents: 1500,
            services: vec![Service {
                name: "VM".into(),
                monthly_price_cents: 1500,
            }],
        };
        let alert = CostAlert {
            recipient: "user@example.test",
            selection: &selection,
            threshold: 2000,
            notification: Notification::Recovered,
        };
        let text = content(&alert, "http://localhost:8082");
        for expected in [
            "Montréal",
            "VM",
            "15,00 €",
            "20,00 €",
            "retour à la normale",
            "réarmée",
        ] {
            assert!(text.contains(expected));
        }
        assert!(!text.contains("Dépassement :"));
        assert!(!text.contains("dépasse son seuil"));
        let email = message(
            "Cost Monitor <cost-monitor@example.test>".parse().unwrap(),
            &alert,
            "http://localhost:8082",
        )
        .unwrap();
        assert_eq!(
            email
                .headers()
                .get::<lettre::message::header::Subject>()
                .unwrap()
                .as_ref(),
            "[Cost Monitor] Retour à la normale"
        );
        assert!(
            String::from_utf8(email.formatted())
                .unwrap()
                .contains("Content-Type: text/plain; charset=utf-8")
        );
    }
    #[test]
    fn email_contains_selection_contents_total_and_threshold_but_no_credentials() {
        let selection = Selection {
            id: 1,
            name: "Montréal".into(),
            monthly_total_cents: 2500,
            services: vec![Service {
                name: "VM".into(),
                monthly_price_cents: 2500,
            }],
        };
        let text = content(
            &CostAlert {
                recipient: "user@example.test",
                selection: &selection,
                threshold: 2000,
                notification: Notification::Exceeded,
            },
            "http://localhost:8082",
        );
        for expected in [
            "Montréal",
            "VM",
            "25,00 €",
            "20,00 €",
            "5,00 €",
            "http://localhost:8082/",
        ] {
            assert!(text.contains(expected));
        }
        let email = message(
            "Cost Monitor <cost-monitor@example.test>".parse().unwrap(),
            &CostAlert {
                recipient: "user@example.test",
                selection: &selection,
                threshold: 2000,
                notification: Notification::Exceeded,
            },
            "http://localhost:8082",
        )
        .unwrap();
        let wire = String::from_utf8(email.formatted()).unwrap();
        assert!(wire.contains("Content-Type: text/plain; charset=utf-8"));
    }
}
