"""Telecharge les extensions d'image du Microsoft Store (HEIF, HEVC, RAW, WebP) depuis le
service de livraison que le Store interroge. Les paquets sont signes par Microsoft :
Add-AppxPackage verifie leur signature avant de les installer.

python recette/extensions.py <dossier>
"""

import html
import re
import sys
import urllib.request
from pathlib import Path
from xml.dom import minidom

FE3 = "https://fe3.delivery.mp.microsoft.com/ClientWebService/client.asmx"

# Categorie Windows Update de chaque extension, lue dans le catalogue du Store
# (displaycatalog.mp.microsoft.com/v7.0/products/<produit>, FulfillmentData)
EXTENSIONS = {
    "Microsoft.HEIFImageExtension": "f4ccc4c1-6e9a-49a0-8f19-46f1717c7ba3",  # 9PMMSR1CGPWG
    "Microsoft.HEVCVideoExtension": "5e60c346-7f52-4a6f-98c0-70fef2dd1d01",  # 9N4WGH0Z6VHQ
    "Microsoft.RawImageExtension": "83edba13-a396-483f-82ab-9ec16f1ba07f",  # 9NCTDW2W1BH8
    "Microsoft.WebpImageExtension": "00c2490a-ef1d-43c9-bdba-b84ecc77d2ea",  # 9PG2DK419DRG
}

# Mises a jour que le service suppose deja installees, sans quoi il ne repond rien
INSTALLEES = (
    "1 2 3 11 19 544 549 2359974 2359977 5169044 8788830 23110993 23110994 54341900 54343656 "
    "59830006 59830007 59830008 60484010 62450018 62450019 62450020 66027979 66053150 97657898 "
    "98822896 98959022 98959023 98959024 98959025 98959026 104433538 104900364 105489019 "
    "117765322 129905029 130040031 132387090 132393049 133399034 138537048 140377312 143747671 "
    "158941041 158941042 158941043 158941044 159123858 159130928 164836897 164847386 164848327 "
    "164852241 164852246 164852252 164852253"
).split()

APPAREIL = (
    "BranchReadinessLevel=CB;CurrentBranch=rs_prerelease;OEMModel=Virtual Machine;FlightRing=Retail;"
    "AttrDataVer=21;SystemManufacturer=Microsoft Corporation;InstallLanguage=en-US;OSUILocale=en-US;"
    "InstallationType=Client;FlightingBranchName=external;FirmwareVersion=Hyper-V UEFI Release v2.5;"
    "SystemProductName=Virtual Machine;OSSkuId=48;FlightContent=Branch;App=WU;"
    "OEMName_Uncleaned=Microsoft Corporation;AppVer=10.0.22621.900;OSArchitecture=AMD64;SystemSKU=None;"
    "UpdateManagementGroup=2;IsFlightingEnabled=1;IsDeviceRetailDemo=0;TelemetryLevel=3;"
    "OSVersion=10.0.22621.900;DeviceFamily=Windows.Desktop;"
)

ENTETE = """<s:Envelope xmlns:a="http://www.w3.org/2005/08/addressing" xmlns:s="http://www.w3.org/2003/05/soap-envelope">
<s:Header>
<a:Action s:mustUnderstand="1">http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService/{action}</a:Action>
<a:MessageID>urn:uuid:175df68c-4b91-41ee-b70b-f2208c65438e</a:MessageID>
<a:To s:mustUnderstand="1">{to}</a:To>
<o:Security s:mustUnderstand="1" xmlns:o="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
<Timestamp xmlns="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd">
<Created>2017-08-05T02:03:05.038Z</Created><Expires>2017-08-05T02:08:05.038Z</Expires>
</Timestamp>
<wuws:WindowsUpdateTicketsToken wsu:id="ClientMSA" xmlns:wsu="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd" xmlns:wuws="http://schemas.microsoft.com/msus/2014/10/WindowsUpdateAuthorization">
<TicketType Name="MSA" Version="1.0" Policy="MBI_SSL"><user></user></TicketType>
</wuws:WindowsUpdateTicketsToken>
</o:Security>
</s:Header>
<s:Body>{corps}</s:Body>
</s:Envelope>"""

COOKIE = """<GetCookie xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
<oldCookie></oldCookie><lastChange>2015-10-21T17:01:07.1472913Z</lastChange>
<currentTime>2017-12-02T00:16:15.217Z</currentTime><protocolVersion>1.40</protocolVersion>
</GetCookie>"""

SYNC = """<SyncUpdates xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
<cookie><Expiration>2045-03-11T02:02:48Z</Expiration><EncryptedData>{cookie}</EncryptedData></cookie>
<parameters>
<ExpressQuery>false</ExpressQuery>
<InstalledNonLeafUpdateIDs>{installees}</InstalledNonLeafUpdateIDs>
<OtherCachedUpdateIDs/>
<SkipSoftwareSync>false</SkipSoftwareSync>
<NeedTwoGroupOutOfScopeUpdates>true</NeedTwoGroupOutOfScopeUpdates>
<FilterAppCategoryIds><CategoryIdentifier><Id>{categorie}</Id></CategoryIdentifier></FilterAppCategoryIds>
<TreatAppCategoryIdsAsInstalled>true</TreatAppCategoryIdsAsInstalled>
<AlsoPerformRegularSync>false</AlsoPerformRegularSync>
<ComputerSpec/>
<ExtendedUpdateInfoParameters>
<XmlUpdateFragmentTypes><XmlUpdateFragmentType>Extended</XmlUpdateFragmentType></XmlUpdateFragmentTypes>
<Locales><string>en-US</string><string>en</string></Locales>
</ExtendedUpdateInfoParameters>
<ClientPreferredLanguages><string>en-US</string></ClientPreferredLanguages>
<ProductsParameters>
<SyncCurrentVersionOnly>false</SyncCurrentVersionOnly>
<DeviceAttributes>{appareil}</DeviceAttributes>
<CallerAttributes>Interactive=1;IsSeeker=0;</CallerAttributes>
<Products/>
</ProductsParameters>
</parameters>
</SyncUpdates>"""

URL = """<GetExtendedUpdateInfo2 xmlns="http://www.microsoft.com/SoftwareDistribution/Server/ClientWebService">
<updateIDs><UpdateIdentity><UpdateID>{update}</UpdateID><RevisionNumber>{revision}</RevisionNumber></UpdateIdentity></updateIDs>
<infoTypes><XmlUpdateFragmentType>FileUrl</XmlUpdateFragmentType><XmlUpdateFragmentType>FileDecryption</XmlUpdateFragmentType></infoTypes>
<deviceAttributes>{appareil}</deviceAttributes>
</GetExtendedUpdateInfo2>"""


def soap(action, corps, securise=False):
    to = FE3 + ("/secured" if securise else "")
    enveloppe = ENTETE.format(action=action, to=to, corps=corps)
    requete = urllib.request.Request(
        to, data=enveloppe.encode(), headers={"Content-Type": "application/soap+xml; charset=utf-8"}
    )
    with urllib.request.urlopen(requete, timeout=120) as r:
        return r.read().decode()


def texte(noeud, nom):
    return noeud.getElementsByTagName(nom)[0].firstChild.nodeValue


def paquets(cookie, categorie):
    """(identifiant de paquet, extension, UpdateID, RevisionNumber) des fichiers de la categorie"""
    corps = SYNC.format(
        cookie=cookie,
        installees="".join(f"<int>{i}</int>" for i in INSTALLEES),
        categorie=categorie,
        appareil=APPAREIL,
    )
    doc = minidom.parseString(html.unescape(soap("SyncUpdates", corps)))
    fichiers = {}
    for u in doc.getElementsByTagName("ExtendedUpdateInfo")[0].getElementsByTagName("Update"):
        for f in u.getElementsByTagName("File"):
            if f.hasAttribute("InstallerSpecificIdentifier") and f.hasAttribute("FileName"):
                ext = f.getAttribute("FileName").rsplit(".", 1)[-1]
                fichiers[texte(u, "ID")] = (f.getAttribute("InstallerSpecificIdentifier"), ext)
    for info in doc.getElementsByTagName("NewUpdates")[0].getElementsByTagName("UpdateInfo"):
        xml = info.getElementsByTagName("Xml")[0]
        identifiant = texte(info, "ID")
        if xml.getElementsByTagName("SecuredFragment") and identifiant in fichiers:
            u = xml.getElementsByTagName("UpdateIdentity")[0]
            yield (*fichiers[identifiant], u.getAttribute("UpdateID"), u.getAttribute("RevisionNumber"))


def version(identifiant):
    return tuple(int(x) for x in identifiant.split("_")[1].split("."))


def main():
    dossier = Path(sys.argv[1])
    dossier.mkdir(parents=True, exist_ok=True)
    cookie = texte(minidom.parseString(soap("GetCookie", COOKIE)), "EncryptedData")
    for nom, categorie in EXTENSIONS.items():
        trouves = list(paquets(cookie, categorie))
        # derniere version de l'extension, et ses dependances pour x64
        extension = max((p for p in trouves if p[0].startswith(nom + "_")), key=lambda p: version(p[0]))
        dependances = [p for p in trouves if not p[0].startswith(nom + "_") and "_x64_" in p[0]]
        for identifiant, ext, update, revision in [extension, *dependances]:
            corps = URL.format(update=update, revision=revision, appareil=APPAREIL)
            doc = minidom.parseString(soap("GetExtendedUpdateInfo2", corps, securise=True))
            # les adresses de 99 caracteres sont celles des cartes de blocs
            url = next(texte(l, "Url") for l in doc.getElementsByTagName("FileLocation") if len(texte(l, "Url")) != 99)
            cible = dossier / f"{identifiant}.{ext}"
            urllib.request.urlretrieve(url, cible)
            print(f"{cible.name} : {cible.stat().st_size // 1024} Ko")


if __name__ == "__main__":
    main()
