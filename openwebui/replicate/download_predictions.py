# Standard library modules
import os
import time
import json
import argparse
from datetime import datetime
from pathlib import Path

# Third-party modules
import replicate
import requests
import pandas as pd
from dotenv import load_dotenv

# Load environment variables from .env file
load_dotenv()

# Replicate API token should be set in .env file
if not os.getenv("REPLICATE_API_TOKEN"):
    raise ValueError("Please set REPLICATE_API_TOKEN in .env file")


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
        # print("Prediction has no output")
        # Add this new code to log predictions with no output as 'removed'
        update_download_log(
            prediction_id=prediction.id,
            filename="no_output",
            prediction_created_at=prediction.created_at,
            status="removed",
        )
        return

    # Create downloads directory if it doesn't exist
    os.makedirs("replicate_downloads", exist_ok=True)

    # Create subdirectory with prediction ID
    download_dir = f"replicate_downloads/{prediction.id}"
    os.makedirs(download_dir, exist_ok=True)

    # Handle different output types
    if isinstance(prediction.output, str):
        try:
            if prediction.output.startswith("http"):
                response = requests.get(prediction.output)
                filename = prediction.output.split("/")[-1]
                filepath = f"{download_dir}/{filename}"
                with open(filepath, "wb") as f:
                    f.write(response.content)
                # Verify file is not corrupted
                if os.path.getsize(filepath) > 0:
                    status = "finished"
                else:
                    status = "corrupted"
            else:
                # Text output
                filename = "output.txt"
                filepath = f"{download_dir}/{filename}"
                with open(filepath, "w") as f:
                    f.write(prediction.output)
                status = "finished"

            update_download_log(
                prediction_id=prediction.id,
                filename=filename,
                prediction_created_at=prediction.created_at,
                status=status,
            )

        except Exception as e:
            print(f"Error downloading {prediction.output}: {str(e)}")
            update_download_log(
                prediction_id=prediction.id,
                filename=filename if "filename" in locals() else "unknown",
                prediction_created_at=prediction.created_at,
                status="corrupted",
            )

    elif isinstance(prediction.output, list):
        for i, item in enumerate(prediction.output):
            try:
                if isinstance(item, str) and item.startswith("http"):
                    response = requests.get(item)
                    filename = item.split("/")[-1]
                    filepath = f"{download_dir}/{filename}"
                    with open(filepath, "wb") as f:
                        f.write(response.content)
                    status = (
                        "finished" if os.path.getsize(filepath) > 0 else "corrupted"
                    )
                elif isinstance(item, dict):
                    filename = f"output_{i}.json"
                    filepath = f"{download_dir}/{filename}"
                    with open(filepath, "w", encoding="utf-8") as f:
                        json.dump(item, f, indent=2, ensure_ascii=False)
                    status = "finished"
                else:
                    filename = f"output_{i}.txt"
                    filepath = f"{download_dir}/{filename}"
                    with open(filepath, "w") as f:
                        f.write(str(item))
                    status = "finished"

                update_download_log(
                    prediction_id=prediction.id,
                    filename=filename,
                    prediction_created_at=prediction.created_at,
                    status=status,
                )

            except Exception as e:
                print(f"Error downloading item {i} from {prediction.id}: {str(e)}")
                update_download_log(
                    prediction_id=prediction.id,
                    filename=filename if "filename" in locals() else f"unknown_{i}",
                    prediction_created_at=prediction.created_at,
                    status="corrupted",
                )
    elif isinstance(prediction.output, dict):
        try:
            filename = "output.json"
            filepath = f"{download_dir}/{filename}"
            with open(filepath, "w", encoding="utf-8") as f:
                json.dump(prediction.output, f, indent=2, ensure_ascii=False)
            status = "finished"

            update_download_log(
                prediction_id=prediction.id,
                filename=filename,
                prediction_created_at=prediction.created_at,
                status=status,
            )
        except Exception as e:
            print(f"Error saving JSON output from {prediction.id}: {str(e)}")
            update_download_log(
                prediction_id=prediction.id,
                filename="output.json",
                prediction_created_at=prediction.created_at,
                status="corrupted",
            )
    else:
        print("Unknown output type")
        print(prediction.output)
        update_download_log(
            prediction_id=prediction.id,
            filename="unknown",
            prediction_created_at=prediction.created_at,
            status="unknown",
        )


def parse_args():
    """Parse command line arguments"""
    parser = argparse.ArgumentParser(description='Download Replicate predictions')
    parser.add_argument(
        '--all-pages',
        action='store_true',
        default=False,
        help='Download predictions from all pages (default: only first page)'
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
        page = replicate.predictions.list() if not cursor else replicate.predictions.list(cursor=cursor)
        
        # Break if no predictions in this page
        if not page.items:
            break
            
        for prediction in page.items:
            # Check if prediction is already completely downloaded
            prediction_files = download_log[download_log['prediction_id'] == prediction.id]
            if not prediction_files.empty and all(prediction_files['status'] == 'finished'):
                print(f"Skipping already downloaded prediction {prediction.id}")
                continue
                
            try:
                print(f"Downloading prediction {prediction.id}...")
                download_prediction(prediction)
                time.sleep(1)  # Add small delay to avoid rate limiting
            except Exception as e:
                print(f"Error downloading prediction {prediction.id}: {str(e)}")
        
        # Get cursor for next page
        cursor = page.next
        
        # Break if no more pages or if we only want the first page
        if not cursor or (not args.all_pages and page_count >= 1):
            break
        
        print(f"Moving to next page...")
    
    print("Download complete!")


if __name__ == "__main__":
    main()
