# Signing the releases

The release workflow (`.github/workflows/release.yml`) signs each platform only when its credentials are configured in the repository. Until then it builds unsigned releases exactly as before.

Secrets and variables live under **Settings → Secrets and variables → Actions** in the GitHub repository.

## macOS: Developer ID and notarization

Result: the app opens with a normal double-click, without the `xattr` step.

1. Join the [Apple Developer Program](https://developer.apple.com/programs/enroll/) (99 USD/year). Enrolment as an individual usually takes a day or two.
2. Create the certificate. On a Mac: Xcode → Settings → Accounts → Manage Certificates → **+** → **Developer ID Application**. Without Xcode: create a certificate signing request in Keychain Access (Certificate Assistant → Request a Certificate From a Certificate Authority), then upload it at [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates/list) and choose **Developer ID Application**.
3. Export it from Keychain Access: select the certificate together with its private key → Export → `.p12`, and set a password.
4. Create an API key for notarization at [App Store Connect → Users and Access → Integrations → Team Keys](https://appstoreconnect.apple.com/access/integrations/api) with the **Developer** role. Download the `.p8` file (possible only once) and note the **Key ID** and the **Issuer ID**.
5. Add these repository **secrets**:

   | Secret | Value |
   | --- | --- |
   | `MACOS_CERT_P12` | `base64 -i certificate.p12 \| pbcopy` |
   | `MACOS_CERT_PASSWORD` | the `.p12` password |
   | `APPLE_API_KEY_P8` | `base64 -i AuthKey_XXXXXXXXXX.p8 \| pbcopy` |
   | `APPLE_API_KEY_ID` | the Key ID |
   | `APPLE_API_ISSUER_ID` | the Issuer ID |

The next tagged release is then signed with the hardened runtime, notarized, stapled and checked with `spctl`. If only the certificate secrets are set, the app is signed but not notarized.

Note for existing Mac users: the first signed version has a different signature from the ad-hoc builds, so macOS asks once for permission to read the saved API secret from the Keychain.

## Windows: SignPath Foundation

Result: the exe carries a valid signature from "SignPath Foundation". SmartScreen can still show a warning for a while after the first signed release, until the download builds reputation.

SignPath Foundation signs open-source projects for free, at its own discretion. Its [conditions](https://signpath.org/terms) ask for an OSI licence, an actively maintained and already released project, "a certain verifiable reputation" for executables, multi-factor authentication for everyone with repository or SignPath access, and manual approval of every release. The code signing policy it requires is in the README.

1. Enable two-factor authentication on the GitHub account if it is not on already.
2. Apply at [signpath.org/apply](https://signpath.org/apply) with the repository URL.
3. Once accepted, in SignPath:
   - add the predefined **GitHub.com** trusted build system to the organization and link it to the project;
   - create a project with slug `nightscout-glance` and a signing policy with slug `release-signing`;
   - set this artifact configuration (the workflow uploads a zip containing the exe):

     ```xml
     <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
       <zip-file>
         <pe-file path="NightscoutGlance.exe" product-name="Nightscout Glance">
           <authenticode-sign/>
         </pe-file>
       </zip-file>
     </artifact-configuration>
     ```

   - create an API token for a user with submitter permission.
4. In the GitHub repository add:

   | Kind | Name | Value |
   | --- | --- | --- |
   | Secret | `SIGNPATH_API_TOKEN` | the API token |
   | Variable | `SIGNPATH_ORGANIZATION_ID` | the SignPath organization ID |
   | Variable | `SIGNPATH_PROJECT_SLUG` | only if it is not `nightscout-glance` |
   | Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | only if it is not `release-signing` |

On each tagged release the Windows job then submits the exe and waits up to two hours for the signing request to be approved in SignPath. After approval it verifies the signature and uploads the signed exe.

## Releasing

```
git tag v0.3.0
git push origin v0.3.0
```

Bump `version` in `Cargo.toml` first; the signing service expects the product version to match the release.
