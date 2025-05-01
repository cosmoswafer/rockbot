# Standard library modules
import os
import time
import json
import argparse
import shutil
from datetime import datetime
from pathlib import Path

# Third-party modules
import replicate
import requests
import pandas as pd
from dotenv import load_dotenv

# Load environment variables from .env file
load_dotenv()

# Supported image extensions
IMAGE_EXTENSIONS = ('.jpg', '.jpeg', '.webp')

# Replicate API token should be set in .env file
if not os.getenv("REPLICATE_API_TOKEN"):
    raise ValueError("Please set REPLICATE_API_TOKEN in .env file")

def save_file(filepath, content, mode="w"):
    """Save content to a file with specified mode (binary or text)"""
    with open(filepath, mode) as f:
        f.write(content)
    return os.path.getsize(filepath) > 0

def download_url(url, download_dir, prediction_id):
    """Download content from a URL and save it to a file in the specified directory"""
    response = requests.get(url)
    filename = url.split("/")[-1]
    filepath = f"{download_dir}/{prediction_id}_{filename}"
    if save_file(filepath, response.content, mode="wb"):
        return filename, "finished"
    else:
        return filename, "corrupted"

def handle_string_output(output, download_dir, prediction_id):
    """Handle string-type output"""
    if output.startswith("http"):
        return download_url(output, download_dir, prediction_id)
    else:
        filename = "output.txt"
        filepath = f"{download_dir}/{prediction_id}_{filename}"
        if save_file(filepath, output, mode="w"):
            return filename, "finished"
        else:
            return filename, "corrupted"

def handle_list_output(output_list, download_dir, prediction_id):
    """Handle list-type output"""
    results = []
    for i, item in enumerate(output_list):
        if isinstance(item, str) and item.startswith("http"):
            results.append(download_url(item, download_dir, prediction_id))
        elif isinstance(item, dict):
            filename = f"output_{i}.json"
            filepath = f"{download_dir}/{prediction_id}_{filename}"
            if save_file(filepath, json.dumps(item, indent=2, ensure_ascii=False), mode="w"):
                results.append((filename, "finished"))
            else:
                results.append((filename, "corrupted"))
        else:
            filename = f"output_{i}.txt"
            filepath = f"{download_dir}/{prediction_id}_{filename}"
            if save_file(filepath, str(item), mode="w"):
                results.append((filename, "finished"))
            else:
                results.append((filename, "corrupted"))
    return results

def handle_dict_output(output_dict, download_dir, prediction_id):
    """Handle dictionary-type output"""
    filename = "output.json"
    filepath = f"{download_dir}/{prediction_id}_{filename}"
    if save_file(filepath, json.dumps(output_dict, indent=2, ensure_ascii=False), mode="w"):
        return filename, "finished"
    else:
        return filename, "corrupted"


def get_download_log():
    """Initialize or load download log CSV file"""
    # Create downloads directory if it doesn't exist
    os.makedirs("replicate_downloads", exist_ok=True)

    log_file = Path("replicate_downloads/download_log.csv")
    if not log_file.exists():
        df = pd.DataFrame(
            columns=[
                "prediction_id",
                "filename",
                "prediction_created_at",
                "download_datetime",
                "status",
            ]
        )
        df.to_csv(log_file, index=False)
    return pd.read_csv(log_file)


def update_download_log(prediction_id, filename, prediction_created_at, status):
    """Update the download log with new entry"""
    log_file = Path("replicate_downloads/download_log.csv")
    df = pd.read_csv(log_file)

    # Check if entry exists
    mask = (df["prediction_id"] == prediction_id) & (df["filename"] == filename)
    new_row = {
        "prediction_id": prediction_id,
        "filename": filename,
        "prediction_created_at": prediction_created_at,
        "download_datetime": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
        "status": status,
    }

    if mask.any():
        # Update existing entry
        df.loc[mask] = pd.Series(new_row)
    else:
        # Add new entry
        df = pd.concat([df, pd.DataFrame([new_row])], ignore_index=True)

    df.to_csv(log_file, index=False)


def download_prediction(prediction):
    """Download the output files from a prediction"""
    if not prediction.output:
        update_download_log(
            prediction_id=prediction.id,
            filename="no_output",
            prediction_created_at=prediction.created_at,
            status="removed",
        )
        return

    # Create download directories
    os.makedirs("replicate_downloads", exist_ok=True)
    #download_dir = f"replicate_downloads/{prediction.id}"
    #os.makedirs(download_dir, exist_ok=True)
    download_dir = "replicate_downloads"

    try:
        if isinstance(prediction.output, str):
            filename, status = handle_string_output(prediction.output, download_dir, prediction.id)
            update_download_log(
                prediction_id=prediction.id,
                filename=filename,
                prediction_created_at=prediction.created_at,
                status=status,
            )

        elif isinstance(prediction.output, list):
            results = handle_list_output(prediction.output, download_dir, prediction.id)
            for filename, status in results:
                update_download_log(
                    prediction_id=prediction.id,
                    filename=filename,
                    prediction_created_at=prediction.created_at,
                    status=status,
                )

        elif isinstance(prediction.output, dict):
            filename, status = handle_dict_output(prediction.output, download_dir, prediction.id)
            update_download_log(
                prediction_id=prediction.id,
                filename=filename,
                prediction_created_at=prediction.created_at,
                status=status,
            )

        else:
            print(f"Unknown output type: {type(prediction.output)}")
            print(prediction.output)
            update_download_log(
                prediction_id=prediction.id,
                filename="unknown",
                prediction_created_at=prediction.created_at,
                status="unknown",
            )

    except Exception as e:
        print(f"Error processing prediction {prediction.id}: {str(e)}")
        filename = getattr(e, 'filename', 'unknown')
        update_download_log(
            prediction_id=prediction.id,
            filename=filename,
            prediction_created_at=prediction.created_at,
            status="corrupted",
        )



def parse_args():
    """Parse command line arguments"""
    parser = argparse.ArgumentParser(description='Download Replicate predictions')
    parser.add_argument(
        '--max-pages',
        type=int,
        default=100,
        help='Maximum number of pages to download (default: 100)'
    )
    parser.add_argument(
        '--stop-latest',
        action='store_true',
        default=False,
        help='Stop when reaching the latest downloaded prediction (status: finished or removed)'
    )
    return parser.parse_args()

def main():
    args = parse_args()
    download_log = get_download_log()

    print("Starting download of predictions...")

    # Start with an empty string cursor for first page
    cursor = ""
    page_count = 0

    while True:
        page_count += 1
        # Get predictions with pagination
        predictions = replicate.predictions.list() if not cursor else replicate.predictions.list(cursor=cursor)

        # Break if no predictions in this page
        if not predictions:
            break

        for prediction in predictions:
            # Check if prediction is already completely downloaded
            prediction_files = download_log[download_log['prediction_id'] == prediction.id]
            if not prediction_files.empty and (all(prediction_files['status'] == 'finished') or all(prediction_files['status'] == 'removed')):
                print(f"Skipping already downloaded prediction {prediction.id}")
                if args.stop_latest:
                    print("Reached latest downloaded prediction, stopping as requested...")
                    return  # Exit the function entirely
                continue

            try:
                print(f"Downloading prediction {prediction.id}...")
                download_prediction(prediction)
                time.sleep(1)  # Add small delay to avoid rate limiting
            except Exception as e:
                print(f"Error downloading prediction {prediction.id}: {str(e)}")

        # Get cursor for next page
        cursor = predictions.next

        # Break if no more pages or if we only want the first page
        if not cursor or page_count >= args.max_pages:
            break

        print(f"Moving to next page...")

    print("Download complete!")


if __name__ == "__main__":
    main()
